//! Pufferfish.playerTouch in the real player's tick, independent of fish AI.
use super::super::{
    Game,
    entity::{Entity, with_entity},
};
use horizon_client::{mobs::Species, scoreboard::teams_allow_damage};
use horizon_core::geometry::Vec3;

impl Game<'_> {
    /// `Pufferfish.playerTouch`: accepted player damage, then poison with this fish as source.
    pub(in crate::game) fn touch_pufferfish(&mut self) {
        if self.remote.is_some() || self.spectator() {
            return;
        }
        let Some(player) = self.player.as_ref().filter(|p| !p.health.dead_or_dying()) else {
            return;
        };
        let mut fish = self
            .mobs
            .iter()
            .filter(|m| m.species() == Species::Pufferfish && m.alive() && m.extra.size > 0)
            .peekable();
        if fish.peek().is_none() {
            return;
        }
        let own = player.bounds();
        // Player.aiStep's contact query; the existing vehicle bounds are read,
        // without moving or simulating that vehicle.
        let vehicle = self.riding.and_then(|id| {
            self.entity_handles().find_map(
                |h| with_entity!(self,h,e => (e.id()==id && !e.is_removed()).then(||e.bounds())),
            )
        });
        let query = vehicle.map_or_else(
            || own.inflate(Vec3::new(1.0, 0.5, 1.0)),
            |b| own.union_bounds(b).inflate(Vec3::new(1.0, 0.0, 1.0)),
        );
        let touching: Vec<_> = fish
            .filter(|m| {
                query.intersects(m.bounds())
                    && teams_allow_damage(self.mob_team_of(m), self.player_team())
            })
            .map(|m| {
                (
                    m.id,
                    m.extra.size,
                    [m.position.x, m.position.y, m.position.z],
                    self.mob_entity(m),
                )
            })
            .collect();
        for (id, puff, at, source) in touching {
            if !self.hurt_player_from(
                (1 + puff) as f32,
                Some(at),
                "minecraft:mob_attack",
                Some(Some(id)),
            ) {
                continue;
            }
            let changes = self.potions.effects.changes;
            self.give_player_effect("minecraft:poison", 60 * puff, 0);
            if self.potions.effects.changes != changes {
                self.trigger_effects_changed_by(Some(source));
            }
        }
    }
}
