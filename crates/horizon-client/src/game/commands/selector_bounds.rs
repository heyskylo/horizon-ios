//! The physical bounds already kept for selector targets. This is not a
//! rendered box or a new source of entity dimensions.
use super::{Game, Target};
use crate::game::entity::Entity;
use horizon_core::geometry::{Aabb, Vec3};

fn extents((lo, hi): ([f64; 3], [f64; 3])) -> Aabb {
    Aabb::new(
        Vec3::new(lo[0], lo[1], lo[2]),
        Vec3::new(hi[0], hi[1], hi[2]),
    )
}

/// `EntitySelectorParser.createAabb`, moved by the selector origin.
pub(super) fn selector_box(origin: [f64; 3], delta: [f64; 3]) -> Aabb {
    let lo = std::array::from_fn(|i| origin[i] + delta[i].min(0.0));
    let hi = std::array::from_fn(|i| origin[i] + (delta[i].max(0.0) + 1.0));
    extents((lo, hi))
}

impl Game<'_> {
    /// `Entity.getBoundingBox`: use each simulated kind's existing physical
    /// box. None means no authoritative box is modeled here yet; the selector
    /// retains its existing point fallback for those kinds.
    pub(super) fn selector_target_bounds(&self, target: Target) -> Option<Aabb> {
        match target {
            Target::Player => self.player.as_ref().map(|p| p.bounds()),
            Target::Guest(conn) => self.command_guest(conn).map(|p| p.bounds()),
            Target::Mob(id) => self.mobs.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Item(id) => self.dropped.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Orb(id) => self.orbs.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Projectile(id) => self
                .creatures
                .projectiles
                .iter()
                .find(|e| e.id == id)
                .map(Entity::bounds),
            Target::Stand(id) => self
                .armor_stands
                .iter()
                .find(|e| e.id == id)
                .map(Entity::bounds),
            Target::Boat(id) => self.boats.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Cart(id) => self.carts.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Tnt(id) => self.tnts.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Falling(id) => self.falling.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Rocket(id) => self.rockets.iter().find(|e| e.id == id).map(Entity::bounds),
            Target::Trident(id) => self
                .tridents
                .iter()
                .find(|e| e.id == id)
                .map(Entity::bounds),
            Target::Frame(id) => self
                .item_frames
                .iter()
                .find(|e| e.id == id)
                .map(|e| extents(e.bounds())),
            Target::Painting(id) => self
                .paintings
                .iter()
                .find(|e| e.id == id)
                .map(|e| extents(e.bounds())),
            Target::Cushion(id) => self
                .cushions
                .iter()
                .find(|e| e.id == id)
                .map(|e| extents(e.bounds())),
            Target::Crystal(id) => self
                .crystals
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.bounds()),
            Target::Other(_)
            | Target::Fangs(_)
            | Target::Dragon(_)
            | Target::Spawner(_)
            | Target::Cloud(_) => None,
            // LeashFenceKnotEntity.recalculateBoundingBox, at the knot's centre.
            Target::Knot(id) => self.natural_knot(id).map(|fence| {
                let knot = horizon_client::mobs::leash::knot_state(fence);
                let (p, half) = (knot.position, knot.width / 2.0);
                Aabb::new(
                    Vec3::new(p.x - half, p.y, p.z - half),
                    Vec3::new(p.x + half, p.y + knot.height, p.z + half),
                )
            }),
        }
    }
}
