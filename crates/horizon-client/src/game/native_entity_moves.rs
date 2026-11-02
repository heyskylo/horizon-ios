//! Moving and turning an entity for native server mods, through an
//! `EntityRef` (docs/native-mods.md, "Entity and level handles", #7387):
//! teleport, rotation, head yaw, facing, size and pushes. Writes move the
//! entity as `/tp` and `/rotate` do, so clients see it as they see those.
//! Reads return `None`, writes false, for a stale handle or on a client.
use super::entity::{Entity as _, EntityHandle, with_entity_mut};
use super::{Game, commands::Target};
use horizon_client::mod_api::EntityRef;
use horizon_core::geometry::Vec3;

/// `Level.isInSpawnableBounds` and `/tp`'s limits: 30,000,000 blocks out,
/// 20,000,000 up or down.
fn in_bounds([x, y, z]: [f64; 3]) -> bool {
    [x, y, z].iter().all(|v| v.is_finite())
        && x.abs() < 30_000_000.0
        && z.abs() < 30_000_000.0
        && y.abs() < 20_000_000.0
}

/// `Direction.fromYRot`: the horizontal direction a yaw faces.
fn facing(yaw: f32) -> &'static str {
    const BY_2D: [&str; 4] = ["south", "west", "north", "east"];
    BY_2D[((f64::from(yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize]
}

impl Game<'_> {
    /// Moves the entity to `pos` in its level (`Entity.teleportTo`), as
    /// `/tp` does: a player is placed there and told, a rider gets off. With
    /// `rotation` (yaw, pitch in degrees) it also turns. False for a position
    /// outside the world's bounds. API @API:entity_moves@.
    pub(super) fn native_entity_teleport_to(
        &mut self,
        entity: EntityRef,
        pos: [f64; 3],
        rotation: Option<[f32; 2]>,
    ) -> bool {
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        if !in_bounds(pos) || rotation.is_some_and(|r| !r.iter().all(|v| v.is_finite())) {
            return false;
        }
        self.teleport_to(t, pos, rotation);
        true
    }
    /// Turns the entity to `yaw` and `pitch` in degrees (`Entity.setYRot`,
    /// `setXRot`), as `/rotate` does: the pitch is clamped to -90..90, and a
    /// player's client is told. API @API:entity_moves@.
    pub(super) fn native_entity_set_rotation(
        &mut self,
        entity: EntityRef,
        yaw: f32,
        pitch: f32,
    ) -> bool {
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(pos) = self.target_position(t) else {
            return false;
        };
        if !yaw.is_finite() || !pitch.is_finite() {
            return false;
        }
        self.teleport_to(t, pos, Some([yaw, pitch]));
        true
    }
    /// The yaw of the entity's head (`Entity.getYHeadRot`): a mob's or a
    /// player's own, the body's yaw for the kinds without a head. API
    /// @API:entity_moves@.
    pub(super) fn native_entity_head_yaw(&self, entity: EntityRef) -> Option<f32> {
        let t = self.native_entity_target(entity)?;
        if let Some(g) = self.native_target_guest(t) {
            return Some(g.head_yaw);
        }
        if let Target::Mob(id) = t {
            return Some(self.mobs.iter().find(|m| m.id == id)?.head_yaw);
        }
        Some(self.target_rotation(t)[0])
    }
    /// Turns a mob's head to `yaw` (`Entity.setYHeadRot`); its body follows
    /// as its AI turns it. False for the other kinds. API @API:entity_moves@.
    pub(super) fn native_entity_set_head_yaw(&mut self, entity: EntityRef, yaw: f32) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        match self.mobs.iter_mut().find(|m| m.id == id) {
            Some(m) if yaw.is_finite() => {
                m.head_yaw = yaw;
                m.needs_sync = true;
                true
            }
            _ => false,
        }
    }
    /// The horizontal direction the entity faces (`Entity.getDirection`):
    /// `north`, `south`, `east` or `west`. API @API:entity_moves@.
    pub(super) fn native_entity_direction(&self, entity: EntityRef) -> Option<&'static str> {
        let t = self.native_entity_target(entity)?;
        Some(facing(self.target_rotation(t)[0]))
    }
    /// The entity's width and height (`Entity.getBbWidth`, `getBbHeight`):
    /// its box's, so a baby's or a scaled entity's own. API
    /// @API:entity_moves@.
    pub(super) fn native_entity_dimensions(&self, entity: EntityRef) -> Option<[f64; 2]> {
        let [min, max] = self.native_entity_bounding_box(entity)?;
        Some([max[0] - min[0], max[1] - min[1]])
    }
    /// Adds `delta` to the entity's velocity (`Entity.push`,
    /// `addDeltaMovement`). False for a player (its client moves it), an
    /// entity Horizon doesn't simulate, or a value that isn't finite. API
    /// @API:entity_moves@.
    pub(super) fn native_entity_push(&mut self, entity: EntityRef, delta: [f64; 3]) -> bool {
        if !delta.iter().all(|v| v.is_finite()) || self.native_entity_target(entity).is_none() {
            return false;
        }
        let Some(h) = self.native_walk_entity(entity.id()) else {
            return false;
        };
        let [x, y, z] = delta;
        with_entity_mut!(self, h, e => {
            let v = e.velocity_mut();
            *v = Vec3::new(v.x + x, v.y + y, v.z + z);
        });
        if let EntityHandle::Mob(i) = h {
            self.mobs[i].needs_sync = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_yaw_faces_vanillas_direction() {
        assert_eq!(super::facing(0.0), "south");
        assert_eq!(super::facing(90.0), "west");
        assert_eq!(super::facing(180.0), "north");
        assert_eq!(super::facing(-90.0), "east");
        assert_eq!(super::facing(-45.0), "south");
        assert_eq!(super::facing(-46.0), "east");
        assert_eq!(super::facing(720.0 + 170.0), "north");
    }

    #[test]
    fn positions_stay_inside_the_worlds_bounds() {
        assert!(super::in_bounds([29_999_999.0, -64.0, 0.0]));
        assert!(!super::in_bounds([30_000_000.0, 64.0, 0.0]));
        assert!(!super::in_bounds([0.0, 20_000_001.0, 0.0]));
        assert!(!super::in_bounds([f64::NAN, 64.0, 0.0]));
    }
}
