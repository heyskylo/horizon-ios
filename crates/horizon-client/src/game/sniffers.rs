//! Sniffers on the game side: the egg a bred pair lays
//! (`Sniffer.spawnChildFromBreeding`), the snifflet that hatches from an
//! egg block (`SnifferEggBlock.tick`) and the dust and sound of digging.
use super::Game;
use horizon_client::{
    items::ItemStack,
    mobs::{Mob, Species, Who, sniffer},
};
use horizon_core::{geometry::Vec3, random::RandomSource};

impl Game<'_> {
    /// The egg item at the parent (`Sniffer.spawnChildFromBreeding`; the
    /// brain already played the plop and dropped the experience).
    pub(super) fn lay_sniffer_egg(&mut self, parent: &Mob) {
        let at = [parent.position.x, parent.position.y, parent.position.z];
        // new ItemEntity(level, x, y, z, stack): the little push is the item's
        // own random's, not the level's.
        self.spawn_item_own(ItemStack::new("minecraft:sniffer_egg", 1), at, 10);
    }
    /// A snifflet where the egg was.
    pub(super) fn hatch_sniffer(&mut self, pos: [i32; 3]) {
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        if let Some(id) = self.spawn_mob_raw("minecraft:sniffer", at, true)
            && let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
        {
            m.extra.age = sniffer::BABY_START_AGE;
            let yaw = self.world.random.next_float() * 360.0 - 180.0;
            m.yaw = yaw;
            m.body_yaw = yaw;
            m.head_yaw = yaw;
        }
    }
    /// The sniffer's client-side effects: `SnifferSoundInstance` when it
    /// starts digging, `Sniffer.emitDiggingParticles` (block dust and the
    /// block's hit sound between the 1.7th and the 6th second of the
    /// digging animation) and `playSearchingSound`.
    pub(super) fn tick_sniffer_effects(&mut self) {
        // Vanilla runs these on each client: a sniffer within 32 blocks of
        // any player in the level (the host and the guests) shows them.
        let players = self.level_players();
        if players.is_empty() {
            return;
        }
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            if m.species() != Species::Sniffer
                || !m.alive()
                || !players
                    .iter()
                    .any(|p| m.position.distance_squared(p.position) <= 32.0 * 32.0)
            {
                continue;
            }
            let id = m.id;
            let at = [m.position.x, m.position.y, m.position.z];
            // playSearchingSound is playLocalSound: silence does not mute it.
            if sniffer::state(m) == sniffer::SEARCHING && m.age.is_multiple_of(20) {
                self.audio
                    .play("minecraft:entity.sniffer.searching", Some(at), 1.0, 1.0);
                continue;
            }
            if !sniffer::digging(m) {
                continue;
            }
            let age = m.age;
            if let Some(ms) = m.anim.states[3].millis(age as f32) {
                if ms == 0 {
                    // Sniffer.onDiggingStart: broadcast entity event 63.
                    self.lan_entity_event(id as i32, 63, at);
                }
                if ms > 1700 && ms < 6000 {
                    let head = sniffer::head_block(&self.mobs[i]);
                    let below = self.world.block(head[0], head[1] - 1, head[2]);
                    // `stateBelow.getRenderShape() != RenderShape.INVISIBLE`.
                    if self.res.facts().get(below).render_shape
                        != horizon_client::blocks::RenderShape::Invisible
                    {
                        if self
                            .particle_sprite(below, [head[0], head[1] - 1, head[2]])
                            .is_some()
                        {
                            let dust = [
                                f64::from(head[0]) + 0.5,
                                f64::from(head[1]) + 0.5 - f64::from(0.65_f32),
                                f64::from(head[2]) + 0.5,
                            ];
                            // Drawn in this window when it has one, and sent to
                            // the players within 32 blocks (the server thread's
                            // window is one of them).
                            self.landing_dust(
                                None,
                                below,
                                Vec3::new(dust[0], dust[1], dust[2]),
                                30,
                                0.0,
                            );
                        }
                        // The hit sound is playLocalSound too: silence does not mute it.
                        if age.is_multiple_of(10) {
                            let hit = self.res.facts().get(below).sounds.hit.clone();
                            self.audio.play(&hit, Some(at), 0.5, 0.5);
                        }
                    }
                }
            }
            // GameEvent.ENTITY_ACTION for the whole dig, outside the dust window.
            if age.is_multiple_of(10) {
                let head = sniffer::head_block(&self.mobs[i]);
                self.post_block_game_event(
                    "minecraft:entity_action",
                    head,
                    Some(Who::Mob(id)),
                    None,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "sniffers_tests.rs"]
mod sniffers_tests;
