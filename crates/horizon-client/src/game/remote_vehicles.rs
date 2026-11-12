//! Boats, minecarts, item frames, armor stands and paintings for a server's
//! client: single player on its server thread, a LAN guest, a player on a
//! dedicated server.
//!
//! The client aims at them like at mobs (`Minecraft.hitResult`, then
//! `ServerboundInteractPacket`), and rides the vehicle the server seats it in
//! (`ClientPacketListener.handleSetEntityPassengersPacket`). A boat is
//! controlled by its rider's client: it ticks here and its moves go to the
//! server (`LocalPlayer.tick`: `ServerboundMoveVehiclePacket`,
//! `AbstractBoat.tick`: `ServerboundPaddleBoatPacket`). So is a saddled horse,
//! donkey, mule or camel: its rider's client moves it and reports the moves,
//! and tells the server of a riding jump (`START_RIDING_JUMP`). Pigs and striders
//! also move on their rider's client when saddled and steered with the right stick.
//! A minecart
//! moves on the server with the rider's input, and the rider sits in it.
use super::{Game, Keys, ray_box};
use crate::Result;
use horizon_client::mobs::{Mob, equine, happy_ghast};
use horizon_client::{keybinds, text::Text};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Serverbound as S};

/// An object under the crosshair: its entity id, how far along the view it
/// was hit, and where (`EntityHitResult.getLocation` minus the entity's
/// position, the interact packet's location).
pub(super) struct ObjectHit {
    pub id: i32,
    pub distance: f64,
    pub location: [f64; 3],
}

impl Game<'_> {
    /// BlockAttachedEntity.isPickable: the nearest fence knot's box the view meets, with its id,
    /// distance and position. Its box is wider than the post inside it, so the crosshair meets it
    /// first and LeashFenceKnotEntity.interact takes the click. The fence block behind only ties
    /// the player's own mobs (LeadItem.useOn), it never takes one back.
    fn remote_knot_hit(&self) -> Option<(i32, f64, [f64; 3])> {
        let (eye, dir) = self.aim()?;
        self.remote_knots
            .iter()
            .filter_map(|(&id, &block)| {
                let knot = horizon_client::mobs::leash::knot_state(block);
                let p = knot.position;
                let half = knot.width / 2.0;
                let bounds = Aabb::new(
                    Vec3::new(p.x - half, p.y, p.z - half),
                    Vec3::new(p.x + half, p.y + knot.height, p.z + half),
                );
                ray_box(eye, dir, self.entity_reach, bounds)
                    .map(|d| (id as i32, d, [p.x, p.y, p.z]))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Whether a use starts on an entity: the crosshair's `EntityHitResult`, or a fence knot.
    /// Only a use: Horizon's attacks still go to the fence behind a knot.
    pub(super) fn remote_use_targets_entity(&self) -> bool {
        self.entity_targeted()
            || self
                .remote_knot_hit()
                .is_some_and(|(_, d, _)| self.before_block(d))
    }

    /// The nearest boat, minecart, item frame, armor stand or painting the
    /// view hits within reach; also End crystals when attacking, and fence knots when using.
    pub(super) fn remote_object_hit(&self, attacking: bool) -> Option<ObjectHit> {
        let (eye, dir) = self.aim()?;
        let at = |d: f64| {
            [
                eye[0] + dir[0] * d,
                eye[1] + dir[1] * d,
                eye[2] + dir[2] * d,
            ]
        };
        let block_centre = |p: [i32; 3]| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + 0.5,
                f64::from(p[2]) + 0.5,
            ]
        };
        let mut hits: Vec<(i32, f64, [f64; 3])> = Vec::new();
        if let Some((i, d)) = self.painting_hit() {
            let p = &self.paintings[i];
            hits.push((p.id as i32, d, block_centre(p.pos)));
        }
        if let Some((i, d)) = self.frame_hit() {
            let f = &self.item_frames[i];
            hits.push((f.id as i32, d, block_centre(f.pos)));
        }
        if let Some((i, d, _)) = self.armor_stand_hit() {
            let a = &self.armor_stands[i];
            let p = a.position;
            hits.push((a.id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d, _)) = self.cushion_hit() {
            let c = &self.cushions[i];
            let p = c.position;
            hits.push((c.id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d)) = self.boat_hit() {
            let p = self.boats[i].position;
            hits.push((self.boats[i].id as i32, d, [p.x, p.y, p.z]));
        }
        if let Some((i, d)) = self.cart_hit() {
            let p = self.carts[i].position;
            hits.push((self.carts[i].id as i32, d, [p.x, p.y, p.z]));
        }
        // Interaction.isPickable: an invisible box the server clicks through
        // to its scripts, for both uses and attacks.
        let reach = self.entity_reach;
        for (&id, i) in &self.interactions {
            if let Some(d) = ray_box(eye, dir, reach, i.bounds()) {
                let p = i.position;
                hits.push((id, d, [p.x, p.y, p.z]));
            }
        }
        if !attacking && let Some((id, d, p)) = self.remote_knot_hit() {
            hits.push((id, d, p));
        }
        if attacking {
            // EndCrystal.isPickable: its 2 x 2 box, with the server's entity
            // id, not special_targets' local projectile target id.
            // Entity.interact passes on a crystal, so right clicks use the item.
            let reach = self.entity_reach;
            for c in self.crystals.iter().filter(|c| c.id != 0) {
                let p = c.position;
                let bounds = Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                );
                if let Some(d) = ray_box(eye, dir, reach, bounds) {
                    hits.push((c.id as i32, d, [p.x, p.y, p.z]));
                }
            }
            // EnderDragonPart.isPickable: the server's dragon hurts by part.
            hits.extend(self.remote_dragon_part_hit(eye, dir, reach));
        }
        let (id, distance, origin) = hits.into_iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
        let point = at(distance);
        Some(ObjectHit {
            id,
            distance,
            location: [
                point[0] - origin[0],
                point[1] - origin[1],
                point[2] - origin[2],
            ],
        })
    }

    /// `ClientPacketListener.handleSetEntityPassengersPacket`: who rides
    /// `vehicle` now. The local player gets in or out; mobs and other
    /// players sit where the server keeps them.
    pub(super) fn remote_set_passengers(&mut self, vehicle: i32, passengers: &[i32]) {
        let Some(me) = self.remote.as_ref().map(|r| r.entity_id) else {
            return;
        };
        let vehicle_id = vehicle as u32;
        if let Some(r) = self.remote.as_mut() {
            r.passenger_vehicles
                .retain(|_, parent| *parent != vehicle_id);
            for &passenger in passengers {
                r.passenger_vehicles.insert(passenger as u32, vehicle_id);
            }
        }
        self.remote_seat_passengers(vehicle_id, passengers);
        let player_controls = passengers.first().is_some_and(|&id| {
            self.remote
                .as_ref()
                .is_some_and(|r| r.level_player(id).is_some())
        });
        if let Some(boat) = self.boats.iter_mut().find(|b| b.id == vehicle_id) {
            boat.paddle_controller = player_controls;
        }

        for m in &mut self.mobs {
            // `Entity.isVehicle` (the saddle reins of a ridden horse): the
            // server sends no rider on the mob itself, only this list.
            if m.id == vehicle_id {
                m.extra.rider = passengers.first().map(|&p| p as u32);
            }
            if m.extra.mount == Some(vehicle_id) && !passengers.contains(&(m.id as i32)) {
                m.extra.mount = None;
                m.extra.passenger = false;
            } else if passengers.contains(&(m.id as i32)) {
                m.extra.mount = Some(vehicle_id);
                m.extra.passenger = true;
            }
        }
        for (id, display) in &mut self.text_displays {
            if passengers.contains(id) {
                display.vehicle = Some(vehicle_id);
            } else if display.vehicle == Some(vehicle_id) {
                display.vehicle = None;
            }
        }
        for a in &mut self.armor_stands {
            if passengers.contains(&(a.id as i32)) {
                a.vehicle = Some(vehicle_id);
            } else if a.vehicle == Some(vehicle_id) {
                a.vehicle = None;
            }
        }
        for (&id, i) in &mut self.interactions {
            if passengers.contains(&id) {
                i.vehicle = Some(vehicle_id);
            } else if i.vehicle == Some(vehicle_id) {
                i.vehicle = None;
            }
        }
        if passengers.contains(&me)
            && let Some(r) = self.remote.as_mut()
        {
            r.first_passenger = passengers.first() == Some(&me);
            r.passenger_index = passengers.iter().position(|&p| p == me).unwrap_or(0);
        }
        if passengers.contains(&me) {
            if self.riding != Some(vehicle_id) {
                self.riding = Some(vehicle_id);
                // ClientPacketListener.handleSetEntityPassengersPacket: boarding says how to get off,
                // on the overlay and to the narrator.
                let key = if self.cushions.iter().any(|c| c.id == vehicle_id) {
                    "mount.sit"
                } else {
                    "mount.onboard"
                };
                let message = Text::translate(
                    key,
                    &[keybinds::display_name(self.settings.keys.get("key.sneak"))],
                );
                self.say_system_now(&message);
                self.titles.set_overlay(message, false);
                if let Some(c) = self.carts.iter_mut().find(|c| c.id == vehicle_id) {
                    c.passenger_yaw_delta = 0.0;
                }
                if let Some(p) = self.player.as_mut() {
                    p.velocity = Vec3::ZERO;
                    p.fall_distance = 0.0;
                }
            }
        } else if self.riding == Some(vehicle_id) {
            // The server takes the player out, then teleports it.
            self.riding = None;
            if let Some(m) = self.mob_index.get_mut(&mut self.mobs, vehicle_id) {
                m.rider = None;
            }
            // HappyGhast.removePassenger: a still timeout of 10 (the server's
            // STAYS_STILL confirms it), and the rider lands on top of the ghast
            // (`place_ghast_under`).
            if let Some(m) = self.mobs.iter_mut().find(|m| {
                m.id == vehicle_id && m.species() == horizon_client::mobs::Species::HappyGhast
            }) {
                m.extra.counter = 10;
                if let Some(r) = self.remote.as_mut() {
                    r.ghast_dismount = Some(vehicle_id);
                }
            }
        }
    }

    /// Camera.alignWithEntity's NewMinecartBehavior branch: sample the
    /// vehicle's curved path at this frame, then apply the player's seat
    /// attachment. Camera's own eye-height interpolation still runs afterwards.
    pub(super) fn remote_minecart_rider_position(&self, partial: f32) -> Option<[f64; 3]> {
        if !self.minecart_improvements() {
            return None;
        }
        let id = self.riding?;
        self.carts
            .iter()
            .find(|c| c.id == id && c.kind == super::minecarts::CartKind::Rideable)?;
        let (mut position, _, _) = self.remote_minecart_lerp(id, partial)?;
        position[1] += super::minecarts::SEAT;
        Some(position)
    }

    /// AbstractBoat.getControllingPassenger: only the first player steers a boat.
    /// A non-remote world retains its existing local movement ownership.
    pub(super) fn controls_boat(&self, id: u32) -> bool {
        self.riding == Some(id) && self.remote.as_ref().is_none_or(|r| r.first_passenger)
    }

    /// The local player's tick in its vehicle, in place of its own movement:
    /// false when it rides nothing.
    pub(super) fn remote_ride(&mut self, keys: Keys, sprint_window: i32) -> Result<bool> {
        let Some(id) = self.riding else {
            return Ok(false);
        };
        // LocalPlayer.aiStep runs its sprint decisions in a vehicle too: `canSprint` and
        // this client's authority stand in for food (`isSprintingPossible`).
        let (can_sprint, local) = self
            .driven_mount()
            .map_or((false, false), |m| (equine::can_sprint(m.species()), true));
        if let Some(p) = self.player.as_mut() {
            p.sprint_input_tick_riding(keys, sprint_window, can_sprint, local)?;
        }
        // Settle an invisible seat riding a moving avatar before reading it.
        // The normal end-of-tick pass alone leaves the local rider a tick behind.
        self.position_riders();
        if let Some(i) = self.mobs.iter().position(|m| m.id == id) {
            // LocalPlayer.aiStep: the jump bar charges while jump is held.
            let jump = self.charge_mount_jump(i, keys);
            if self.drives(&self.mobs[i]) {
                self.drive_mount(i, keys, jump);
            }
            // Another animal moves on the server with the rider's input
            // (PlayerInput); the rider sits where `positionRider` puts it.
            self.position_rider();
            return Ok(true);
        }
        // Generic entities are valid vehicles too (plugins use displays as seats).
        // Keep riding set so sendChanges sends rotation, never a walking position.
        if !self.cushions.iter().any(|c| c.id == id)
            && !self.boats.iter().any(|b| b.id == id)
            && !self.carts.iter().any(|c| c.id == id)
        {
            let index = std::collections::HashMap::new();
            if let Some(at) = self.passenger_riding_position(id, None, None, &index) {
                let before = self
                    .text_displays
                    .get(&(id as i32))
                    .map(|d| d.seat().1)
                    .or_else(|| {
                        self.armor_stands.iter().find(|a| a.id == id).map(|a| {
                            let delta = a.previous.subtract(a.position);
                            at + glam::DVec3::new(delta.x, delta.y, delta.z)
                        })
                    });
                if let Some(p) = self.player.as_mut() {
                    let scale = p.dimensions.width
                        / horizon_client::pose::Pose::Standing.dimensions(1.0).width;
                    let (seat, previous) = generic_player_seat(at, before.unwrap_or(at), scale);
                    let previous = if before.is_some() {
                        previous
                    } else {
                        p.position
                    };
                    self.previous_position = if self.posed_rider != self.riding {
                        seat
                    } else {
                        previous
                    };
                    self.posed_rider = self.riding;
                    p.position = seat;
                    p.velocity = Vec3::ZERO;
                    p.fall_distance = 0.0;
                }
                return Ok(true);
            }
        }
        let seat = if let Some(c) = self.cushions.iter().find(|c| c.id == id) {
            // The seat does not move; the rider sits on the cushion.
            Some(c.seat())
        } else if let Some(i) = self.boats.iter().position(|b| b.id == id) {
            // AbstractBoat.tick on the controlling client: steer and move.
            // It steers with the input `LocalPlayer.rideTick` latched on the
            // previous tick, so the paddle packet describes the motion the
            // server is sent; this tick's keys are latched after it ticked.
            let mut b = self.boats[i].clone();
            if self.controls_boat(id) {
                b = self.tick_boat(b, true);
                b.input = [keys.left, keys.right, keys.forward, keys.backward];
            }
            // positionRider: the seat turns with the boat; clampRotation,
            // which moves `yRotO` (the held lead's old look) with the view.
            self.yaw += b.delta_rotation;
            let delta = (self.yaw - b.yaw + 180.0).rem_euclid(360.0) - 180.0;
            let clamped = delta.clamp(-105.0, 105.0) - delta;
            self.yaw += clamped;
            self.rope_rot_old.0 += clamped;
            self.boats[i] = b;
            // In front of a mob that shares the boat.
            Some(self.player_boat_seat(i))
        } else if self.carts.iter().any(|c| c.id == id) {
            // Minecart.positionRider runs on the client after the cart tick.
            // The offset belongs to the cart, so switching vehicles cannot
            // transfer the previous vehicle's pending turn.
            let rotate =
                self.minecart_improvements() && self.settings.file.bool("rotateWithMinecart");
            if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
                c.passenger_yaw_delta = 0.0;
                if rotate && c.kind == super::minecarts::CartKind::Rideable {
                    c.rotate_passenger(&mut self.yaw);
                }
            }
            // A minecart: the server moves it, the rider sits in it.
            self.carts.iter().find(|c| c.id == id).map(|c| {
                c.position
                    .add_vector(Vec3::new(0.0, super::minecarts::SEAT, 0.0))
            })
        } else if let Some(at) =
            self.passenger_riding_position(id, None, None, &std::collections::HashMap::new())
        {
            let scale = self.player.as_ref().map_or(1.0, |p| {
                p.dimensions.width / horizon_client::pose::Pose::Standing.dimensions(1.0).width
            });
            Some(Vec3::new(at.x, at.y - f64::from(0.6_f32 * scale), at.z))
        } else {
            // Entity.baseTick: a passenger whose vehicle is gone stops
            // riding it. Nothing with the id is in the level any more (it
            // died, or the world it rode in was left), so the seat goes.
            self.riding = None;
            return Ok(false);
        };
        if let Some(p) = self.player.as_mut() {
            // The tick a rider takes its seat is a teleport (`Entity.moveTo`,
            // which sets the old position too): the camera must not sweep from
            // where the player stood to the cushion, boat or cart. The ticks
            // after it move with the vehicle and interpolate as usual.
            let first = self.posed_rider != self.riding;
            self.posed_rider = self.riding;
            if let Some(v) = seat {
                self.previous_position = if first { v } else { p.position };
                p.position = v;
            } else {
                self.previous_position = p.position;
            }
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
        Ok(true)
    }

    /// `LocalPlayer.tick` for a passenger: the boat it controls goes to the
    /// server every tick with its paddles (the rider's own position does not).
    pub(super) fn remote_vehicle_packets(&self) -> Vec<S> {
        if let Some(m) = self.driven_mount() {
            return vec![mount_move(m)];
        }
        let Some(b) = self
            .riding
            .filter(|&id| self.controls_boat(id))
            .and_then(|id| self.boats.iter().find(|b| b.id == id))
        else {
            return Vec::new();
        };
        vec![
            S::MoveVehicle(play::MoveVehicleRequest {
                position: [b.position.x, b.position.y, b.position.z],
                yaw: b.yaw,
                pitch: 0.0,
                on_ground: b.on_ground,
            }),
            S::PaddleBoat(play::PaddleBoat {
                left: b.paddling[0],
                right: b.paddling[1],
            }),
        ]
    }

    /// `ClientPacketListener.handleMoveVehicle`: the server put the boat or
    /// the mount the player drives back.
    pub(super) fn remote_move_vehicle(&mut self, position: [f64; 3], yaw: f32) {
        let at = Vec3::new(position[0], position[1], position[2]);
        if let Some(id) = self.driven_mount().map(|m| m.id)
            && let Some(m) = self.mob_index.get_mut(&mut self.mobs, id)
        {
            m.previous = at;
            m.position = at;
            m.yaw = yaw;
            m.previous_yaw = yaw;
            m.velocity = Vec3::ZERO;
            return;
        }
        let Some(b) = self
            .riding
            .filter(|&id| self.controls_boat(id))
            .and_then(|id| self.boats.iter_mut().find(|b| b.id == id))
        else {
            return;
        };
        b.previous = at;
        b.position = at;
        b.yaw = yaw;
        b.previous_yaw = yaw;
        b.velocity = Vec3::ZERO;
    }

    /// `AbstractBoat.handleEntityEvent`: 71 and 72, a bubble column dragging
    /// the boat down or throwing it up (`handleBubbleColumnEffect`, which the
    /// client applies to the boat it drives).
    pub(super) fn remote_boat_event(&mut self, id: i32, event: i8) {
        let drag_down = event == super::boats::BUBBLE_DOWN_EVENT;
        if !drag_down && event != super::boats::BUBBLE_UP_EVENT {
            return;
        }
        if let Some(i) = self.boats.iter().position(|b| b.id == id as u32) {
            self.handle_bubble_column_effect(i, drag_down);
        }
    }

    /// `AbstractBoat.tick`: the part that reaches bubble columns, for every
    /// boat on a client: `status`, the two `applyEffectsFromBlocks` (a boat
    /// above a column may splash; the boat the player drives is lifted or
    /// dragged by a column it is inside) and `tickBubbleColumn`'s wobble from
    /// the synched bubble time, followed by the oars on boats this client does not drive.
    pub(super) fn tick_remote_boat_bubbles(&mut self) {
        for i in 0..self.boats.len() {
            let driven = self.controls_boat(self.boats[i].id);
            if !driven {
                // `AbstractBoat.tick`: the non-authoritative copy carries no
                // cached motion into the next time this player drives it.
                self.boats[i].velocity = Vec3::ZERO;
                self.boats[i].delta_rotation = 0.0;
                self.refresh_boat_status(i);
            }
            let touches = self.boat_bubble_columns(i);
            for _ in 0..2 {
                for &column in &touches {
                    if column.surface {
                        if self.boat_splash_roll(i) {
                            self.boat_bubble_splash(i);
                        }
                    } else if driven {
                        self.boat_inside_bubble_column(i, column);
                    }
                }
            }
            self.boats[i].tick_bubble_wobble();
            if !driven {
                Self::tick_boat_paddles(&mut self.boats[i], &mut self.audio);
            }
        }
    }

    /// Whether this client moves `m`, the mount the player rides: as its
    /// first passenger (`AbstractHorse.getControllingPassenger`), on its
    /// saddle (`isLocalInstanceAuthoritative`).
    fn drives(&self, m: &Mob) -> bool {
        self.riding == Some(m.id)
            && client_moves(
                m,
                self.remote.as_ref().is_some_and(|r| r.first_passenger),
                self.steers_mount(m),
            )
    }

    fn steers_mount(&self, m: &Mob) -> bool {
        super::mounts::steering_item(
            m.species(),
            self.held().map(|s| s.item.as_ref()),
            self.offhand.as_ref().map(|s| s.item.as_ref()),
        )
    }

    /// The mount this client moves, if the player rides one.
    pub(super) fn driven_mount(&self) -> Option<&Mob> {
        let id = self.riding?;
        self.mobs.iter().find(|m| m.id == id && self.drives(m))
    }

    /// `LivingEntity.travelRidden`, then `LocalPlayer.aiStep`, on the
    /// controlling client. The vehicle ticks before its passenger
    /// (`ClientLevel.tickNonPassenger`), so the mount moves (`tickRidden`,
    /// then `travel`) on what the rider's previous tick left: `xxa`/`zza` from
    /// `applyInput` (the keys through `modifyInput`: ×0.98, square movement),
    /// the sprint and the jump `onPlayerJump` set. Only then does this tick's
    /// aiStep read the keys, send a released jump (`sendRidingJump`) and run
    /// `onPlayerJump`, for the mount's next tick. Moving on this tick's keys
    /// put every ridden move a tick ahead of the server's view of the input.
    fn drive_mount(&mut self, i: usize, keys: Keys, jump: f32) {
        let steer = self.steers_mount(&self.mobs[i]);
        let (yaw, pitch) = (self.yaw, self.pitch);
        // The first ridden tick reads what the player's last walking tick applied.
        let walked = self.player.as_ref().map_or([0.0; 2], |p| p.applied_input);
        let m = &mut self.mobs[i];
        let last = m.rider.unwrap_or(equine::Rider {
            strafe: walked[0],
            forward: walked[1],
            ..Default::default()
        });
        // tickRidden reads the controller's look as it is now.
        m.rider = Some(equine::Rider {
            yaw,
            pitch,
            steer,
            client: false,
            ..last
        });
        let mut mob = self.mobs.remove(i);
        // #15294: the fall, lava or a wall it runs into hurt the server's
        // horse, not this copy (`Entity.hurt` on a client level).
        mob.extra.client_side = true;
        let mut random = self.world.random.clone();
        let mut events = Vec::new();
        {
            let world = self.mob_world_base(&[]);
            mob.tick(&world, &mut random, &mut events);
        }
        // Only the server's packets take the mount away.
        mob.removed = false;
        self.world.random = random;
        self.mobs.insert(i, mob);
        // LocalPlayer.aiStep, after the vehicle's tick.
        if jump > 0.0
            && let Some(id) = self.remote.as_ref().map(|r| r.entity_id)
        {
            self.send(riding_jump(id, self.mount.jump_scale));
        }
        let [strafe, forward] = horizon_client::input::modify_input(keys.direction(), None, None);
        // The rider's sprint state, not the key: Camel.getRiddenSpeed reads
        // `controller.isSprinting()`, which keeps the double-tapped sprint
        // after the key is let go (`shouldStopRunSprinting` decides the end).
        let sprinting = self.player.as_ref().is_some_and(|p| p.sprinting);
        let m = &mut self.mobs[i];
        if jump > 0.0 && equine::is_equine(m.species()) {
            // `onPlayerJump`: the mount slides on the rider's keys and rears
            // while the server does the same.
            equine::on_player_jump(m);
        }
        // playerJumpPendingScale waits for the ground (the tick spends it).
        let pending = m.rider.map_or(0.0, |r| r.jump);
        m.rider = Some(equine::Rider {
            strafe,
            forward,
            yaw,
            pitch,
            jump: if jump > 0.0 { jump } else { pending },
            steer,
            sprint: sprinting,
            jumping: keys.jump,
            client: false,
        });
        // #4821: footstep, gallop, breathe, splash and landing sounds from mob.tick
        let _ = self.handle_mob_events(events);
    }
}

/// Whether a player's client moves `m`, the mount it rides
/// (`isLocalInstanceAuthoritative`): a saddled horse, donkey, mule or camel
/// it is the first passenger of (`AbstractHorse.getControllingPassenger`),
/// or a harnessed adult Happy Ghast it is the first passenger of
/// (`HappyGhast.getControllingPassenger`: wearing body armor, player on
/// top, not on a still timeout — the rider steers it with `travelRidden`
/// on their own client, and the mount goes to the server as `MoveVehicle`).
fn client_moves(m: &Mob, first_passenger: bool, steer: bool) -> bool {
    if !first_passenger || !m.alive() {
        return false;
    }
    if equine::moved_by_rider(m.species()) && m.extra.saddled {
        return !matches!(
            m.species(),
            horizon_client::mobs::Species::Pig | horizon_client::mobs::Species::Strider
        ) || steer;
    }
    m.species() == horizon_client::mobs::Species::HappyGhast
        && happy_ghast::harnessed_adult(m)
        && !happy_ghast::on_still_timeout(m)
}

/// `LocalPlayer.sendRidingJump`: START_RIDING_JUMP with
/// `floor(getJumpRidingScale() * 100)`, as `onPlayerJump` gets it.
fn riding_jump(player: i32, scale: f32) -> S {
    S::PlayerCommand(play::PlayerCommand {
        id: player,
        action: 3,
        data: (scale * 100.0).floor() as i32,
    })
}

/// `ServerboundMoveVehiclePacket.fromEntity`: where the mount this client
/// moves is, and how it is turned.
fn mount_move(m: &Mob) -> S {
    S::MoveVehicle(play::MoveVehicleRequest {
        position: [m.position.x, m.position.y, m.position.z],
        yaw: m.yaw,
        pitch: m.head_pitch,
        on_ground: m.on_ground,
    })
}

/// Avatar's VEHICLE attachment scales independently of the vehicle's seat.
fn generic_player_seat(at: glam::DVec3, before: glam::DVec3, scale: f32) -> (Vec3, Vec3) {
    let offset = 0.6 * f64::from(scale);
    (
        Vec3::new(at.x, at.y - offset, at.z),
        Vec3::new(before.x, before.y - offset, before.z),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_player_on_block_display_and_interaction() {
        for kind in ["minecraft:block_display", "minecraft:interaction"] {
            let at = glam::DVec3::new(10.5, -59.5, 0.5);
            let before = glam::DVec3::new(10.0, -59.0, 0.5);
            let (seat, previous) = super::generic_player_seat(at, before, 1.0);
            assert_eq!(
                seat,
                horizon_core::geometry::Vec3::new(10.5, -60.1, 0.5),
                "{kind}"
            );
            assert_eq!(
                previous,
                horizon_core::geometry::Vec3::new(10.0, -59.6, 0.5),
                "{kind}"
            );
            let (scaled, _) = super::generic_player_seat(at, before, 2.0);
            assert_eq!(scaled.y, -60.7, "{kind}");
        }
    }

    use super::*;
    use horizon_client::mobs::MobTypes;

    #[test]
    #[ignore = "requires prepared resources and the vanilla mount-control reference"]
    fn steering_mount_control_matches_vanilla() {
        use super::super::{
            Audio, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
        };
        use horizon_client::{items::ItemStack, motion::PlayerMotion};
        use horizon_protocol::connection::Pipe;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let reference = std::env::var_os("HORIZON_MOUNT_CONTROL_REFERENCE").unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(reference).unwrap()).unwrap();
        assert_eq!(rows.len(), 32, "missing vanilla control cases");
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!("no menu handoff"),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = ExecutionRole::Client;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let (client, _server) = Pipe::pair();
        game.connect_local(client, "SteeringTest").unwrap();
        game.remote.as_mut().unwrap().joined = true;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 300.0, 0.5)).unwrap());
        let types = MobTypes::load(&root).unwrap();
        for row in rows {
            let kind = row["kind"].as_str().unwrap();
            let t = std::sync::Arc::new(types.types[&format!("minecraft:{kind}")].clone());
            let mut m = Mob::new(7, t, Vec3::new(0.5, 300.0, 0.5), 0.0);
            if row["saddled"].as_bool().unwrap() {
                m.set_saddle(Some(ItemStack::new("minecraft:saddle", 1)));
            }
            let stick = if kind == "pig" {
                "minecraft:carrot_on_a_stick"
            } else {
                "minecraft:warped_fungus_on_a_stick"
            };
            let wrong = if kind == "pig" {
                "minecraft:warped_fungus_on_a_stick"
            } else {
                "minecraft:carrot_on_a_stick"
            };
            let hand = row["hand"].as_str().unwrap();
            game.inventory[game.selected] = match hand {
                "main" => Some(ItemStack::new(stick, 1)),
                "wrong" => Some(ItemStack::new(wrong, 1)),
                _ => None,
            };
            game.offhand = (hand == "off").then(|| ItemStack::new(stick, 1));
            game.mobs.clear();
            game.mobs.push(m);
            game.riding = Some(7);
            game.remote.as_mut().unwrap().first_passenger = row["first"].as_bool().unwrap();
            let controlled = row["controlled"].as_bool().unwrap();
            assert_eq!(game.driven_mount().is_some(), controlled, "{row}");
            let packets = game.remote_vehicle_packets();
            assert_eq!(packets.len(), usize::from(controlled), "{row}");
            if controlled {
                assert!(matches!(packets[0], S::MoveVehicle(_)), "{row}");
                let before = game.mobs[0].position;
                assert!(game.remote_ride(Keys::default(), 0).unwrap());
                let m = &game.mobs[0];
                assert_eq!(
                    f64::from(m.ai.zza),
                    row["forward"].as_f64().unwrap(),
                    "{row}"
                );
                assert!(m.position.z > before.z, "{row}: mount did not move forward");
                match &game.remote_vehicle_packets()[0] {
                    S::MoveVehicle(p) => assert_eq!(
                        p.position,
                        [m.position.x, m.position.y, m.position.z],
                        "{row}"
                    ),
                    packet => panic!("wrong movement packet: {packet:?}"),
                }
            }
            // The server reads the same held-item requirement, then waits for
            // this controlling client's MoveVehicle rather than traveling twice.
            if row["first"].as_bool().unwrap() {
                assert!(game.tick_mount_riding(Keys::default()));
                game.mobs[0].rider.as_mut().unwrap().client = true;
            } else {
                game.mobs[0].rider = None;
            }
            assert_eq!(
                equine::moved_by_client(&game.mobs[0]),
                !row["server_simulates"].as_bool().unwrap(),
                "{row}"
            );
            if controlled {
                let mut server_mount = game.mobs[0].clone();
                let before = server_mount.position;
                let mut random = game.world.random.clone();
                let mut events = Vec::new();
                server_mount.tick(&game.mob_world_base(&[]), &mut random, &mut events);
                assert_eq!(
                    server_mount.position, before,
                    "server traveled twice: {row}"
                );
            }
        }
    }

    /// A mob of the prepared resources' type `kind` (None without them).
    fn mob(kind: &str) -> Option<Mob> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let types = MobTypes::load(&root).ok()?;
        let t = std::sync::Arc::new(types.types.get(kind)?.clone());
        Some(Mob::new(7, t, Vec3::new(1.5, 64.0, -2.5), 0.0))
    }

    /// #2920: on a vanilla server the rider's client moves a saddled horse,
    /// donkey, mule or camel; a pig without its stick, a llama, an unsaddled horse or a second
    /// passenger's ride is the server's.
    #[test]
    fn the_rider_moves_a_saddled_horse_on_its_client() {
        for kind in ["horse", "donkey", "mule", "camel", "pig", "llama"] {
            let Some(mut m) = mob(&format!("minecraft:{kind}")) else {
                return;
            };
            assert!(!client_moves(&m, true, false), "{kind} without a saddle");
            m.set_saddle(Some(horizon_client::items::ItemStack::new(
                "minecraft:saddle",
                1,
            )));
            let horse = matches!(kind, "horse" | "donkey" | "mule" | "camel");
            assert_eq!(client_moves(&m, true, false), horse, "{kind}");
            assert!(
                !client_moves(&m, false, false),
                "{kind} behind another rider"
            );
        }
    }

    /// #4107: on a vanilla server the rider's client moves a harnessed adult
    /// Happy Ghast (`getControllingPassenger`); an unharnessed one, a baby
    /// or a second passenger's ride is the server's.
    #[test]
    fn the_rider_moves_a_harnessed_ghast_on_its_client() {
        let Some(mut m) = mob("minecraft:happy_ghast") else {
            return;
        };
        assert!(!client_moves(&m, true, false), "ghast without a harness");
        m.extra.body = Some("minecraft:white_happy_ghast_harness".into());
        assert!(client_moves(&m, true, false), "harnessed adult");
        assert!(!client_moves(&m, false, false), "behind another rider");
        m.extra.age = -1;
        assert!(!client_moves(&m, true, false), "baby ghast");
    }

    /// The mount ticks before its rider (`ClientLevel.tickNonPassenger`): it
    /// moves on the input the rider's previous tick applied (`zza`, the key
    /// ×0.98 through `LocalPlayer.modifyInput`), so a press reaches it a tick
    /// late and a release a tick late too, as the server's anticheats expect.
    #[test]
    #[ignore = "requires prepared resources (HORIZON_TEST_RESOURCES)"]
    fn a_driven_horse_moves_on_the_riders_last_tick_input() {
        use super::super::{
            Audio, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
        };
        use horizon_client::{items::ItemStack, motion::PlayerMotion};
        use horizon_protocol::connection::Pipe;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!("no menu handoff"),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = ExecutionRole::Client;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let (client, _server) = Pipe::pair();
        game.connect_local(client, "RiderTest").unwrap();
        game.remote.as_mut().unwrap().joined = true;
        game.remote.as_mut().unwrap().first_passenger = true;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 300.0, 0.5)).unwrap());
        let types = MobTypes::load(&root).unwrap();
        let t = std::sync::Arc::new(types.types["minecraft:horse"].clone());
        let mut horse = Mob::new(7, t, Vec3::new(0.5, 300.0, 0.5), 0.0);
        horse.set_saddle(Some(ItemStack::new("minecraft:saddle", 1)));
        game.mobs.push(horse);
        game.riding = Some(7);
        let forward = Keys {
            forward: true,
            ..Keys::default()
        };
        let zza = |game: &mut Game, keys| {
            assert!(game.remote_ride(keys, 0).unwrap());
            game.mobs[0].ai.zza
        };
        assert_eq!(zza(&mut game, forward), 0.0, "the press waits a tick");
        assert_eq!(zza(&mut game, forward), 0.98, "modifyInput's 0.98");
        assert_eq!(
            zza(&mut game, Keys::default()),
            0.98,
            "the release waits a tick"
        );
        assert_eq!(zza(&mut game, Keys::default()), 0.0);
    }

    /// #2920: the client reports where it moved the mount, and a released
    /// jump with the bar's percentage (`sendRidingJump`).
    #[test]
    fn the_rider_sends_its_mounts_moves_and_jumps() {
        let Some(mut m) = mob("minecraft:horse") else {
            return;
        };
        (m.yaw, m.head_pitch, m.on_ground) = (90.0, 10.0, true);
        match mount_move(&m) {
            S::MoveVehicle(p) => {
                assert_eq!(p.position, [1.5, 64.0, -2.5]);
                assert_eq!((p.yaw, p.pitch, p.on_ground), (90.0, 10.0, true));
            }
            other => panic!("{other:?}"),
        }
        match riding_jump(42, 0.75) {
            S::PlayerCommand(p) => assert_eq!((p.id, p.action, p.data), (42, 3, 75)),
            other => panic!("{other:?}"),
        }
    }
}

#[cfg(test)]
mod plugin_seat_tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn local_rider_uses_attachments_and_follows_a_moving_seat_chain() {
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        use horizon_client::{mobs::Mob, motion::PlayerMotion, sprint::Keys};
        use horizon_core::geometry::Vec3;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
        let mut stand =
            horizon_client::entities::ArmorStand::new(42, Vec3::new(2.0, 64.0, 3.0), 0.0);
        stand.marker = true;
        game.armor_stands.push(stand);
        game.riding = Some(42);
        assert!(game.remote_ride(Keys::default(), 0).unwrap());
        let close = |actual: f64, expected: f64| {
            assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}")
        };
        // EntityAttachments: zero-height marker minus Avatar's 0.6 attachment.
        close(game.player.as_ref().unwrap().position.y, 63.4);
        game.armor_stands[0].marker = false;
        game.armor_stands[0].small = true;
        game.remote_ride(Keys::default(), 0).unwrap();
        close(game.player.as_ref().unwrap().position.y, 64.3875);
        game.armor_stands[0].small = false;
        game.remote_ride(Keys::default(), 0).unwrap();
        close(game.player.as_ref().unwrap().position.y, 65.375);
        let kind = game.res.player_type().clone().unwrap();
        game.mobs
            .push(Mob::new(43, kind, Vec3::new(5.0, 70.0, 6.0), 0.0));
        game.armor_stands[0].marker = true;
        game.armor_stands[0].vehicle = Some(43);
        for at in [Vec3::new(5.0, 70.0, 6.0), Vec3::new(12.0, 72.0, -4.0)] {
            game.mobs[0].position = at;
            // No manual position_riders call: local ride must resolve the chain itself.
            game.remote_ride(Keys::default(), 0).unwrap();
            let p = game.player.as_ref().unwrap().position;
            close(p.x, at.x);
            close(p.z, at.z);
            close(p.y, at.y + 1.2);
        }
        // Direct riding and a scaled avatar share the same attachment calculation.
        game.riding = Some(43);
        game.mobs[0].extra.model_scale = 1.5;
        game.remote_ride(Keys::default(), 0).unwrap();
        close(game.player.as_ref().unwrap().position.y, 74.1);
        game.mobs.clear();
        game.armor_stands.clear();
        assert!(!game.remote_ride(Keys::default(), 0).unwrap());
        assert_eq!(game.riding, None);
    }
}
