//! Enderman leave-block attempts need the authoritative block simulation.
use super::Game;
use horizon_client::{Result, mobs::Species};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};
use horizon_world::{
    level::{LevelAccess, flags},
    registry::BlockStateId,
};

impl Game<'_> {
    /// `Enderman.hurtServer` teleports inside the hurt (`teleport()`,
    /// `repeatedlyTryToTeleport()`), and so does `Shulker.hurtServer`
    /// (`teleportSomewhere()`, `hitByShulkerBullet()`). `Mob::damage` has no
    /// world, so it leaves them pending (an enderman's tries in `counter2`, a
    /// shulker's flags); a hurt the game layer deals runs them here, right
    /// after it, not on the mob's next AI step.
    pub(super) fn settle_hurt_teleports(&mut self) -> Result<()> {
        let pending: Vec<u32> = self
            .mobs
            .iter()
            .filter(|m| {
                (m.species() == Species::Enderman && m.extra.counter2 > 0)
                    || horizon_client::mobs::shulker::teleport_pending(m)
            })
            .map(|m| m.id)
            .collect();
        for id in pending {
            // What `LivingEntity.hurtServer` raised comes first, where the
            // mob was hurt and from its random's next draws: the hurt
            // sound, `InfestedMobEffect.onMobHurt`'s silverfish.
            let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
                continue;
            };
            let hurt = std::mem::take(&mut self.mobs[i].outbox);
            if !hurt.is_empty() {
                self.handle_mob_events(hurt)?;
            }
            let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
                continue;
            };
            let shulker = self.mobs[i].species() == Species::Shulker;
            // A shulker's teleport reads the mobs around it (`noCollision`,
            // the crowding count of `hitByShulkerBullet`), itself included as
            // in its own tick; an enderman's reads only blocks.
            let uuid = self.level.player_uuid;
            let others: Vec<_> = if shulker {
                self.mobs
                    .iter()
                    .map(|m| super::creatures::seen(m, uuid))
                    .collect()
            } else {
                Vec::new()
            };
            // The world is read while the mob moves: take it out, then put it
            // back where it was (the mob order stays).
            let mut mob = self.mobs.swap_remove(i);
            let mut events = Vec::new();
            // `hitByShulkerBullet` draws its crowding check from the level's random.
            let mut level_random = self.world.random.clone();
            {
                let world = self.mob_world_base(&others);
                if shulker {
                    horizon_client::mobs::shulker::run_pending_teleports(
                        &mut mob,
                        &world,
                        &mut level_random,
                        &mut events,
                    );
                } else {
                    horizon_client::mobs::enderman::run_pending_teleports(
                        &mut mob,
                        &world,
                        &mut events,
                    );
                }
            }
            self.world.random = level_random;
            self.mobs.push(mob);
            let last = self.mobs.len() - 1;
            self.mobs.swap(i, last);
            self.handle_mob_events(events)?;
        }
        Ok(())
    }
    /// `EndermanLeaveBlockGoal.tick`: normalize, check, place, post
    /// BLOCK_PLACE with the enderman and the placed state, then clear.
    pub(super) fn enderman_place_block(
        &mut self,
        mob: u32,
        pos: [i32; 3],
        carried: BlockStateId,
        occupants: &[Aabb],
    ) -> Result<()> {
        // Runtime ids identify the actual mob in this level. An obsolete attempt
        // must not clear another mob's hand or a subsequently changed stack.
        if !self.mobs.iter().any(|m| {
            m.id == mob
                && m.alive()
                && m.species() == Species::Enderman
                && m.extra.carried == Some(carried)
        }) {
            return Ok(());
        }
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        let area = Aabb::new(
            Vec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])),
            Vec3::new(
                f64::from(pos[0] + 1),
                f64::from(pos[1] + 1),
                f64::from(pos[2] + 1),
            ),
        );
        let placed = self.with_level(|b, level, ticks| {
            let target = level.block_state(at);
            let below = level.block_state(at.offset(0, -1, 0));
            // Must run even when eligibility fails: updateShape can schedule
            // ticks and invoke native hooks on the real level.
            let state = b.update_from_neighbour_shapes(level, ticks, at, carried);
            let support = level.facts.get(below);
            if !level.facts.is_air(target)
                || level.facts.is_air(below)
                || &*support.name == "minecraft:bedrock"
                || !support.full_collision
                || occupants.iter().any(|bounds| bounds.intersects(area))
            {
                return None;
            }
            // AIR is a legitimate normalized state. setBlock may report no
            // change for it; the eligible operation still empties the hand.
            b.set_block(level, ticks, at, state, flags::UPDATE_ALL);
            Some(state)
        });
        let Some(state) = placed else {
            return self.process_level_events();
        };
        self.post_block_game_event(
            "minecraft:block_place",
            pos,
            Some(horizon_client::mobs::Who::Mob(mob)),
            Some(state),
        );
        if let Some(m) = self.mobs.iter_mut().find(|m| {
            m.id == mob && m.species() == Species::Enderman && m.extra.carried == Some(carried)
        }) {
            m.extra.carried = None;
        }
        self.process_level_events()
    }
}
