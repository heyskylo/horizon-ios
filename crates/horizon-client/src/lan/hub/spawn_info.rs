//! `CommonPlayerSpawnInfo` for a guest: the login, respawn and travel packets share it.

use super::Hub;
use horizon_protocol::packets::play;

impl Hub {
    /// `CommonPlayerSpawnInfo`, with the player's `portalCooldown`.
    pub(super) fn spawn_info(
        &self,
        i: usize,
        portal_cooldown: i32,
        sea_level: i32,
    ) -> play::SpawnInfo {
        let g = &self.guests[i];
        play::SpawnInfo {
            dimension_type: self.dimension_type,
            dimension: self.config.dimension.clone(),
            seed: horizon_core::biome::obfuscate_seed(self.config.seed),
            game_type: g.game_mode,
            previous_game_type: None,
            debug: self.debug_world,
            flat: self.flat_world,
            // Player.getLastDeathLocation.
            last_death: g
                .last_death
                .as_ref()
                .map(|(dimension, pos)| play::GlobalPos {
                    dimension: dimension.clone(),
                    pos: *pos,
                }),
            portal_cooldown,
            sea_level,
        }
    }
}
