//! Enderman leave-block attempts need the authoritative block simulation.
use super::Game;
use horizon_client::{
    Result,
    mobs::{Species, Who},
};
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
        let candidate = self.with_level(|b, level, ticks| {
            let target = level.block_state(at);
            let below = level.block_state(at.offset(0, -1, 0));
            // Failed candidates still run normalization and its scheduled ticks.
            let state = b.update_from_neighbour_shapes(level, ticks, at, carried);
            let support = level.facts.get(below);
            if !level.facts.is_air(target)
                || level.facts.is_air(below)
                || &*support.name == "minecraft:bedrock"
                || !support.full_collision
                || !b.can_survive(level, ticks, at, state)
            {
                return None;
            }
            Some(state)
        });
        // Raw entity queries occur after neighbor-shape side effects. Mobs
        // retain the exact boxes/removal state seen by this goal's tick.
        self.process_level_events()?;
        if let Some(state) = candidate
            && !occupants.iter().any(|bounds| bounds.intersects(area))
            && !self.enderman_non_mob_in(area)
        {
            // AIR is a legitimate normalized state; an eligible no-op set
            // still posts the event and clears the hand, as the native goal.
            self.with_level(|b, level, ticks| {
                b.set_block(level, ticks, at, state, flags::UPDATE_ALL)
            });
            self.post_block_game_event(
                "minecraft:block_place",
                pos,
                Some(Who::Mob(mob)),
                Some(state),
            );
            if let Some(m) = self.mobs.iter_mut().find(|m| {
                m.id == mob && m.species() == Species::Enderman && m.extra.carried == Some(carried)
            }) {
                m.extra.carried = None;
            }
        }
        self.process_level_events()
    }

    /// Raw EntityGetter candidates, not building blockers. Mobs use the goal's
    /// per-mob snapshot; these other lists are queried after prior queued events.
    fn enderman_non_mob_in(&self, area: Aabb) -> bool {
        use super::commands::Target;
        use super::entity::{Entity as _, entity_lists_of, for_each_entity};
        let mut occupied = false;
        for_each_entity!(entity_lists_of!(self), entity => {
            if !matches!(entity.selector_target(),Some(Target::Mob(_)))
                && !entity.is_removed() && entity.bounds().intersects(area) {
                occupied=true;
            }
        });
        if occupied {
            return true;
        }
        if self
            .player
            .as_ref()
            .filter(|_| !self.spectator())
            .is_some_and(|p| p.bounds().intersects(area))
        {
            return true;
        }
        if self.lan_acting.is_some() {
            if self
                .lan_building_boxes
                .boxes(self.dim)
                .into_iter()
                .any(|(lo, hi)| {
                    Aabb::new(
                        Vec3::new(lo[0], lo[1], lo[2]),
                        Vec3::new(hi[0], hi[1], hi[2]),
                    )
                    .intersects(area)
                })
            {
                return true;
            }
        } else if self.lan.as_ref().is_some_and(|host| {
            host.hub.guests.iter().any(|g| {
                host.hub.here(g)
                    && super::building_players::guest_body_present(g)
                    && !g.spectator()
                    && g.bounds().intersects(area)
            })
        }) {
            return true;
        }
        let box_hits = |(lo, hi): ([f64; 3], [f64; 3])| {
            Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            )
            .intersects(area)
        };
        if self.item_frames.iter().any(|e| box_hits(e.bounds()))
            || self.paintings.iter().any(|e| box_hits(e.bounds()))
            || self.crystals.iter().any(|e| e.bounds().intersects(area))
        {
            return true;
        }
        // OtherEntity has no authoritative dimensions except the simulated
        // marker's zero-size box; retain its existing selector point fallback.
        if self
            .others
            .iter()
            .any(|e| Aabb::new(e.position, e.position).intersects(area))
        {
            return true;
        }
        let sized = |at: [f64; 3], width: f64, height: f64| {
            super::point_box(at, width, height).intersects(area)
        };
        if self
            .fangs
            .list
            .iter()
            .any(|e| sized(e.pos, 0.5, f64::from(0.8f32)))
            || self
                .ominous_spawners
                .iter()
                .any(|e| sized(e.position, 0.25, 0.25))
            || self
                .potions
                .thrown_entities()
                .iter()
                .any(|(_, p, _)| sized([p.x, p.y, p.z], 0.25, 0.25))
            || self
                .breath_clouds
                .iter()
                .any(|e| sized(e.at, f64::from(e.radius * 2.0), 0.5))
            || self
                .potions
                .clouds
                .iter()
                .any(|e| sized(e.at, f64::from(e.radius * 2.0), 0.5))
            || self
                .hook
                .as_ref()
                .is_some_and(|e| e.bounds().intersects(area))
        {
            return true;
        }
        if self.lan.as_ref().is_some_and(|host| {
            host.hub
                .guests
                .iter()
                .filter(|g| host.hub.here(g))
                .any(|g| {
                    host.play
                        .get(&g.conn)
                        .and_then(|p| p.hook.as_ref())
                        .is_some_and(|e| e.bounds().intersects(area))
                })
        }) {
            return true;
        }
        self.dragon.as_ref().is_some_and(|d| {
            sized([d.position.x, d.position.y, d.position.z], 16.0, 8.0)
                || (0..8).any(|part| d.part_box(part).intersects(area))
        })
    }
}
