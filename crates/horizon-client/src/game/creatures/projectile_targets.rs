//! What a projectile can strike: the mobs, the player, the dragon's parts and
//! End crystals, and the non-living entities that are pickable (boats,
//! minecarts, item frames).
use super::super::Game;
use horizon_client::mobs::Who;
use horizon_core::geometry::{Aabb, Vec3};

impl Game<'_> {
    /// The boats, minecarts and item frames a projectile can strike.
    pub(in crate::game) fn vehicle_targets(&self) -> Vec<(Who, Aabb)> {
        let mut targets: Vec<(Who, Aabb)> = self
            .boats
            .iter()
            .map(|b| (Who::Mob(b.id), b.bounds()))
            .collect();
        targets.extend(self.carts.iter().map(|c| (Who::Mob(c.id), c.bounds())));
        // Item frames are pickable too (`BlockAttachedEntity.isPickable`).
        targets.extend(self.item_frames.iter().map(|f| {
            let (lo, hi) = f.bounds();
            (
                Who::Mob(f.id),
                Aabb::new(
                    Vec3::new(lo[0], lo[1], lo[2]),
                    Vec3::new(hi[0], hi[1], hi[2]),
                ),
            )
        }));
        targets
    }
    /// What a projectile can hit: the mobs, the player, the dragon's parts and
    /// the End crystals (under reserved ids, game/dragon.rs), and the vehicles
    /// and item frames.
    pub(in crate::game) fn projectile_targets(&self) -> Vec<(Who, Aabb)> {
        let mut targets: Vec<(Who, Aabb)> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| (Who::Mob(m.id), m.bounds()))
            .collect();
        if let Some(p) = self.player_view()
            && p.alive
        {
            targets.push((Who::Player, p.bounds));
        }
        targets.extend(
            self.special_targets()
                .into_iter()
                .map(|(id, b)| (Who::Mob(id), b)),
        );
        // Boats, minecarts (`VehicleEntity.isPickable`) and item frames are
        // pickable for every projectile, so every kind shares this one list.
        targets.extend(self.vehicle_targets());
        targets
    }
}
