//! Vehicles with nothing of their own to steer or seat with: armor stands, display entities, markers,
//! interactions and the other entities Horizon keeps as their compound. `RideCommand.mount` forces the ride
//! (`startRiding(vehicle, true, true)`), so any of them takes a rider; the rider then sits at the vehicle's
//! default passenger attachment and gets off on sneak, as vanilla's `Entity` does for a vehicle that
//! overrides neither.
use super::{Game, Keys};
use horizon_core::geometry::Vec3;
use horizon_world::nbt::Tag;

/// `Avatar.DEFAULT_VEHICLE_ATTACHMENT`: a player sits this far below the vehicle's passenger point.
const PLAYER_VEHICLE_ATTACHMENT: f64 = 0.6;

/// Such a vehicle: where it is, and how tall and wide its box is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Seat {
    pub position: Vec3,
    pub height: f64,
    pub width: f32,
}

impl Seat {
    /// `Entity.positionRider` for a player: the default passenger attachment `(0, height, 0)`
    /// (`EntityAttachments.createDefault`), less the player's vehicle attachment.
    pub fn player_seat(&self) -> Vec3 {
        Vec3::new(
            self.position.x,
            self.position.y + self.height - PLAYER_VEHICLE_ATTACHMENT,
            self.position.z,
        )
    }
    /// `Entity.getDismountLocationForPassenger`: `(getX(), getBoundingBox().maxY, getZ())`.
    pub fn dismount(&self) -> Vec3 {
        Vec3::new(
            self.position.x,
            self.position.y + self.height,
            self.position.z,
        )
    }
}

/// The box height of an entity Horizon keeps as its compound, from its type's `EntityDimensions` (the
/// passenger attachment sits on top of it): displays and markers are `sized(0, 0)`, an interaction is as
/// tall as its `height` (1 by default), an area effect cloud half a block.
fn other_height(kind: &str, tag: Option<&Tag>) -> (f64, f32) {
    match kind {
        "minecraft:interaction" => {
            let field = |key: &str| match tag.and_then(|t| t.get(key)) {
                Some(Tag::Float(v)) => Some(f64::from(f32::from_bits(*v))),
                _ => None,
            };
            (
                field("height").unwrap_or(1.0),
                field("width").unwrap_or(1.0) as f32,
            )
        }
        "minecraft:area_effect_cloud" => (0.5, 6.0),
        _ => (0.0, 0.0),
    }
}

impl Game<'_> {
    /// The armor stand or kept entity `id` as a vehicle, if it is one.
    pub(super) fn seat(&self, id: u32) -> Option<Seat> {
        if let Some(a) = self.armor_stands.iter().find(|a| a.id == id) {
            // ArmorStand: sized(0.5F, 1.975F), half that when small, nothing as a
            // marker; the passenger point is the float height widened to double.
            let (height, width) = if a.marker {
                (0.0, 0.0)
            } else if a.small {
                (f64::from(0.9875_f32), 0.25)
            } else {
                (f64::from(1.975_f32), 0.5)
            };
            return Some(Seat {
                position: a.position,
                height,
                width,
            });
        }
        let o = self.others.iter().find(|o| o.id == id)?;
        let (height, width) = other_height(&o.kind, self.persist_raw(id));
        Some(Seat {
            position: o.position,
            height,
            width,
        })
    }

    /// Seats the player in place on such a vehicle (`ServerPlayer.startRiding`: at once, as a teleport).
    pub(super) fn board_seat(&mut self, id: u32, seat: Seat) {
        let at = seat.player_seat();
        if let Some(player) = self.player.as_mut() {
            player.position = at;
            player.velocity = Vec3::ZERO;
            player.fall_distance = 0.0;
            player.teleported = true;
        }
        self.previous_position = at;
        self.start_riding(id);
    }

    /// `LivingEntity.stopRiding` off such a vehicle: `dismountVehicle` on top of it. A vehicle that is
    /// gone lets the rider go where it is.
    pub(super) fn get_off_seat(&mut self) {
        let Some(id) = self.riding else {
            return;
        };
        if !self.native_may_leave(id) {
            return;
        }
        self.riding = None;
        let Some(seat) = self.seat(id) else {
            return;
        };
        let at = seat.dismount();
        if let Some(p) = self.player.as_mut() {
            self.previous_position = p.position;
            p.position = at;
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
    }

    /// `Player.rideTick` on such a vehicle: keep the seat (the vehicle may have been moved), get off on
    /// sneak. The seat, or None once off.
    pub(super) fn tick_seat_riding(&mut self, keys: Keys) -> Option<Vec3> {
        let seat = self.seat(self.riding?)?;
        if keys.shift {
            self.get_off_seat();
            return None;
        }
        let at = seat.player_seat();
        if let Some(p) = self.player.as_mut() {
            self.previous_position = p.position;
            p.position = at;
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
        Some(at)
    }

    /// `Entity.positionRider` for the mobs riding such a vehicle: its passenger attachment less the
    /// rider's own vehicle attachment. A server sends no position for a passenger, so every client seats
    /// it here.
    pub(super) fn seat_static_mobs(&mut self, riding: &super::passengers::Riders) {
        for (&vehicle, js) in riding {
            let Some(seat) = self.seat(vehicle) else {
                continue;
            };
            for &j in js {
                let m = &mut self.mobs[j];
                let offset = horizon_client::mobs::riders::vehicle_attachment(m, seat.width);
                let at = Vec3::new(
                    seat.position.x,
                    seat.position.y + seat.height - offset,
                    seat.position.z,
                );
                m.position = at;
                m.previous = at;
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.extra.passenger = true;
                m.ai.nav.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_sits_on_the_passenger_point_and_gets_off_on_top() {
        // A display: sized(0, 0), its passenger point at its position.
        let display = Seat {
            position: Vec3::new(1.5, 64.0, -2.5),
            height: 0.0,
            width: 0.0,
        };
        assert_eq!(display.player_seat(), Vec3::new(1.5, 63.4, -2.5));
        assert_eq!(display.dismount(), Vec3::new(1.5, 64.0, -2.5));
        // An armor stand: its passenger point on top of its 1.975 box.
        let stand = Seat {
            position: Vec3::new(0.5, 64.0, 0.5),
            height: 1.975,
            width: 0.5,
        };
        assert!((stand.player_seat().y - 65.375).abs() < 1e-9);
        assert!((stand.dismount().y - 65.975).abs() < 1e-9);
    }

    #[test]
    fn kept_entities_are_as_tall_as_their_type() {
        assert_eq!(other_height("minecraft:item_display", None), (0.0, 0.0));
        assert_eq!(other_height("minecraft:marker", None), (0.0, 0.0));
        assert_eq!(other_height("minecraft:interaction", None), (1.0, 1.0));
        assert_eq!(
            other_height("minecraft:area_effect_cloud", None),
            (0.5, 6.0)
        );
    }
}
