//! Dropped items and experience orbs: their tick (`tick_items`, `tick_dropped_items`,
//! `tick_experience`), the stacks a destroyed container lets fall, and spawning and merging orbs.
use super::*;

impl Game<'_> {
    pub(super) fn tick_items(&mut self) {
        // EntityTickEvent.Pre for the items about to tick, and the Post after them.
        let mut round = self.native_tick_round();
        if round.listening() {
            let ticking = self.entity_ticking_area();
            let merging = self.remote.is_none();
            let mut i = 0;
            while let Some(item) = self.dropped.get(i) {
                let (id, x, z) = (
                    item.id,
                    item.position.x.floor() as i32,
                    item.position.z.floor() as i32,
                );
                if !(merging && self.joined_during_entity_pass(id)) && ticking(x, z) {
                    let kind = native_ticks::TickKind::Fixed("minecraft:item");
                    self.native_tick_pre_batch(&mut round, id, kind);
                }
                i += 1;
            }
        }
        self.tick_dropped_items(&mut round);
        self.native_tick_post_batch(round);
    }
    /// An item that joined during this tick's entity pass: it first ticks on
    /// the next one.
    pub(super) fn joined_during_entity_pass(&self, id: u32) -> bool {
        id > self.ticked_entities_through || self.contact_drops.contains(&id)
    }
    pub(super) fn tick_dropped_items(&mut self, round: &mut native_ticks::TickRound) {
        let mut items = std::mem::take(&mut self.dropped);
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        let mut removed = vec![false; items.len()];
        let mut cells = horizon_client::entities::ItemCells::new(&items);
        // A LAN or server guest only shows the host's items.
        let merging = self.remote.is_none();
        let mut obstacles =
            horizon_client::entities::ObstacleGrid::new(self.item_entity_collisions());
        let mut nearby = Vec::new();
        let mut bubble_tops = Vec::new();
        // `ItemUtils.onContainerDestroyed`: what a destroyed item held, and
        // where it lay, for adding to the level once its own tick is done.
        let mut destroyed: Vec<(ItemStack, Vec3)> = Vec::new();
        for i in 0..items.len() {
            if removed[i] {
                // Merged into an earlier item before its own tick: it never ticked.
                round.not_ticked(items[i].id);
                continue;
            }
            let item = &mut items[i];
            // `addFreshEntity` during the entity pass: the item ticks from the
            // next tick on (`EntityTickList`).
            if merging && self.joined_during_entity_pass(item.id) {
                continue;
            }
            if !ticking(
                item.position.x.floor() as i32,
                item.position.z.floor() as i32,
            ) || round.canceled(item.id)
            {
                continue;
            }
            // ItemEntity.tick's noPhysics (servers only): out of a block
            // placed on it, towards the closest open side (#1252).
            let no_physics = merging && item.inside_block(&self.world, self.collision);
            // `moveTowardsClosestSpace` draws the speed from the item's own random.
            let escape_speed = no_physics.then(|| item.random.next_float() * 0.2 + 0.1);
            // `getEntityCollisions(entity, bounds.expandTowards(movement))`: the boxes around
            // where the item can reach this tick (a block of slack for the next velocity).
            let reach = Vec3::new(
                item.velocity.x.abs() + 1.0,
                item.velocity.y.abs() + 1.0,
                item.velocity.z.abs() + 1.0,
            );
            obstacles.near(item.bounds().inflate(reach), &mut nearby);
            let mut entered = Vec::new();
            let (alive, discarded) = self.tick_item_with_block_contacts(
                item,
                escape_speed,
                &nearby,
                false,
                &mut |splash| entered.push(splash),
            );
            // Entity.doWaterSplashEffect: the server plays the sound (broadcast
            // to clients). Its addParticle is empty.
            for splash in entered {
                self.apply_item_splash(splash);
            }
            if discarded {
                removed[i] = true;
                cells.remove(i, item.previous);
                continue;
            }
            if merging && !no_physics {
                self.item_landing_event(item.id, item.previous, item.position, item.on_ground);
                self.entity_powder_snow_particles(
                    item.previous,
                    item.position,
                    item.bounds(),
                    item.velocity,
                    false,
                );
            }
            cells.moved(i, item.previous, item.position);
            // Entity.handleOnAboveBubbleColumn: the server sends the particles of
            // the surface of a bubble column the item is at the top of.
            if merging {
                bubble_tops.extend(horizon_client::motion::bubble_column_surfaces(
                    &self.world,
                    self.pack.assets.registry,
                    self.res.facts(),
                    self.collision,
                    item.bounds(),
                ));
            }
            // ItemEntity.tick: merge every 2 ticks while moving between
            // blocks, every 40 otherwise, before the lifetime check.
            let block = |v: Vec3| (v.x.floor(), v.y.floor(), v.z.floor());
            let rate = if block(item.previous) == block(item.position) {
                40
            } else {
                2
            };
            if merging
                && (alive || item.age == 6000 && item.position.y > -128.0)
                && item.ticks.is_multiple_of(rate)
            {
                let max_stack = |stack: &ItemStack| self.stack_max_size(stack).max(1);
                horizon_client::entities::merge_with_neighbours(
                    &mut items,
                    &mut removed,
                    &mut cells,
                    i,
                    &max_stack,
                );
            }
            let item = &mut items[i];
            let mut alive = alive || (item.age < 6000 && item.position.y > -128.0);
            if merging && !removed[i] && item.age >= 6000 && item.position.y > -128.0 {
                let mut extra_life = 0;
                self.native_item_expire(item.id, &item.stack, &mut extra_life);
                if extra_life > 0 {
                    item.age = 6000 - extra_life.min(32766);
                    alive = true;
                } else if self.serving.is_some() || self.options.dedicated.is_some() {
                    self.native_item_despawn(
                        item.id,
                        &item.stack.item,
                        item.stack.count as i32,
                        [item.position.x, item.position.y, item.position.z],
                    );
                }
            }
            if !alive || removed[i] {
                removed[i] = true;
                cells.remove(i, item.position);
                continue;
            }
            let environment = item.environment_tick(
                &self.world,
                self.res.facts(),
                self.pack.assets.registry,
                world_items!(self),
            );
            if environment.should_play_lava_hurt_sound(item.ticks) {
                let pitch = 2.0 + self.world.random.next_float() * 0.4;
                self.audio.play(
                    "minecraft:entity.generic.burn",
                    Some([item.position.x, item.position.y, item.position.z]),
                    0.4,
                    pitch,
                );
            }
            if !environment.alive {
                removed[i] = true;
                cells.remove(i, item.position);
                for stack in environment.spilled {
                    destroyed.push((stack, item.position));
                }
            }
        }
        for pos in bubble_tops {
            self.bubble_column_particles(pos);
        }
        let mut removed = removed.into_iter();
        items.retain(|_| !removed.next().unwrap_or(false));
        let Some(p) = self.player.as_ref() else {
            self.restore_item_batch(items);
            self.spawn_destroyed_contents(destroyed);
            return;
        };
        if p.health.dead_or_dying() || self.spectator() {
            self.restore_item_batch(items);
            self.spawn_destroyed_contents(destroyed);
            return;
        }
        let w = f64::from(p.dimensions.width) / 2.0;
        let (min, max) = (
            [
                p.position.x - w - 1.0,
                p.position.y - 0.5,
                p.position.z - w - 1.0,
            ],
            [
                p.position.x + w + 1.0,
                p.position.y + f64::from(p.dimensions.height) + 0.5,
                p.position.z + w + 1.0,
            ],
        );
        let feet = [p.position.x, p.position.y, p.position.z];
        let own = self.level.ensure_player_uuid();
        let mut kept = Vec::with_capacity(items.len());
        for mut item in items {
            let b = item.bounds();
            let touching = b.min_position().x < max[0]
                && b.max_position().x > min[0]
                && b.min_position().y < max[1]
                && b.max_position().y > min[1]
                && b.min_position().z < max[2]
                && b.max_position().z > min[2];
            // ItemEntity.playerTouch: no pickup delay, and the target (if any) is this player.
            if touching && item.pickup_delay == 0 && item.target.is_none_or(|t| t == own) {
                if self.native_pickup_refused(&item.stack, item.thrower, item.position) {
                    item.pickup_delay = 10;
                    kept.push(item);
                    continue;
                }
                let before = item.stack.count;
                let kind = item.stack.item.clone();
                let thrower = item.thrower;
                match self.give(item.stack.clone()) {
                    None => {
                        self.award_stat("picked_up", &kind, before as i32);
                        if let Some(thrower) = thrower {
                            self.trigger_thrown_picked_up_by_player(&item.stack, thrower);
                        }
                        self.add_pickup(
                            pickups::Taken::Item(Box::new(item)),
                            pickups::Collector::Player,
                        );
                        let pitch = ((self.world.random.next_float()
                            - self.world.random.next_float())
                            * 0.7
                            + 1.0)
                            * 2.0;
                        self.audio
                            .play("minecraft:entity.item.pickup", Some(feet), 0.2, pitch);
                        continue;
                    }
                    Some(rest) => {
                        if rest.count < before {
                            self.award_stat("picked_up", &kind, (before - rest.count) as i32);
                            if let Some(thrower) = thrower {
                                self.trigger_thrown_picked_up_by_player(&rest, thrower);
                            }
                            self.add_pickup(
                                pickups::Taken::Item(Box::new(item.clone())),
                                pickups::Collector::Player,
                            );
                            // handleTakeItemEntity: the random pitch of a full pickup.
                            let pitch = ((self.world.random.next_float()
                                - self.world.random.next_float())
                                * 0.7
                                + 1.0)
                                * 2.0;
                            self.audio
                                .play("minecraft:entity.item.pickup", Some(feet), 0.2, pitch);
                        }
                        item.stack = rest;
                    }
                }
            }
            kept.push(item);
        }
        self.restore_item_batch(kept.into());
        self.spawn_destroyed_contents(destroyed);
    }
    /// `ItemUtils.onContainerDestroyed`: each stack a destroyed item held
    /// becomes an item entity where that item lay (`new ItemEntity(level, x,
    /// y, z, stack)`), nudged by 0.2 less 0.1 per axis and pickable at once.
    pub(super) fn spawn_destroyed_contents(&mut self, destroyed: Vec<(ItemStack, Vec3)>) {
        for (stack, at) in destroyed {
            let r = &mut self.world.random;
            let motion = [
                r.next_double() * 0.2 - 0.1,
                r.next_double() * 0.2 - 0.1,
                r.next_double() * 0.2 - 0.1,
            ];
            self.spawn_item(stack, [at.x, at.y, at.z], motion, 0);
        }
    }
    /// `ExperienceOrb.award`: orbs of the largest sizes; each draws the id
    /// `tryMergeToExisting` compares from the level random, then moves with
    /// its own random.
    pub(super) fn spawn_orbs(&mut self, at: [f64; 3], value: u32) {
        self.spawn_orbs_toward(at, [0.0; 3], value);
    }
    /// `ExperienceOrb.awardWithDirection`: `award` whose fresh orbs start
    /// `roughly` that way (`new ExperienceOrb(level, pos, roughly, value)`).
    pub(super) fn spawn_orbs_toward(&mut self, at: [f64; 3], roughly: [f64; 3], mut value: u32) {
        while value > 0 {
            let v = ExperienceOrb::split(value);
            value -= v;
            let merge_id = self.world.random.next_int_bound(40).unwrap_or(0);
            if !self.merge_new_orb(at, v, merge_id) {
                self.add_orb_toward(at, roughly, v);
            }
        }
    }
    /// `ExperienceOrb.tryMergeToExisting`: an orb of the same value within a
    /// 1 block box of `at`, whose id is a multiple of 40 from `merge_id`, takes
    /// the new one as one more in its count (and restarts its age); otherwise
    /// a fresh orb spawns.
    pub(super) fn award_orb(&mut self, at: [f64; 3], value: u32, merge_id: i32) {
        if !self.merge_new_orb(at, value, merge_id) {
            self.add_orb(at, value);
        }
    }
    /// `tryMergeToExisting`: true when an orb took the new one.
    pub(super) fn merge_new_orb(&mut self, at: [f64; 3], value: u32, merge_id: i32) -> bool {
        let centre = Vec3::new(at[0], at[1], at[2]);
        let half = Vec3::new(0.5, 0.5, 0.5);
        let area = Aabb::new(centre.subtract(half), centre.add_vector(half));
        // A server's orbs are its own; a LAN or server guest only shows them.
        let mut target = None;
        if self.remote.is_none() {
            for i in 0..self.orbs.len() {
                let o = &self.orbs[i];
                if !o.bounds().intersects(area) {
                    continue;
                }
                let vanilla = o.can_merge(merge_id as u32, value as i32);
                let (id, orb_value) = (o.id, o.points());
                if self.orb_merges(id, orb_value, 0, value, vanilla) {
                    target = Some(i);
                    break;
                }
            }
        }
        if let Some(i) = target {
            self.orbs[i].count += 1;
            self.orbs[i].age = 0;
            true
        } else {
            false
        }
    }
    /// Whether orb `into` takes orb `from` (0: a spawning orb of `from_value`),
    /// given vanilla's `canMerge` answer: the mods may flip it, then cancel the merge.
    pub(super) fn orb_merges(
        &mut self,
        into: u32,
        into_value: u32,
        from: u32,
        from_value: u32,
        vanilla: bool,
    ) -> bool {
        let dimension = self.dim.id();
        let mut can_merge = vanilla;
        self.native_experience_orb_can_merge(
            dimension,
            into,
            from,
            into_value,
            from_value,
            &mut can_merge,
        );
        if !can_merge {
            return false;
        }
        let mut canceled = false;
        self.native_experience_orb_merge(dimension, into, from, from_value, &mut canceled);
        !canceled
    }
    /// `ExperienceOrb.tick`'s scanForMerges for one orb, in its own turn
    /// before the move: it absorbs the orbs that `canMerge` within 0.5 blocks
    /// of its box, adding their counts and keeping the smaller age. Server
    /// only, as the caller checks. An orb absorbed here is gone before its
    /// own turn finds it.
    pub(super) fn merge_orb(&mut self, i: usize) {
        // `tickCount % 20 == 1`, with `tickCount` the increment `tick` adds
        // later in the turn: at the scan's moment the stored count is one
        // behind, so the cadence reads `tickCount - 1` a whole 20.
        if !self.orbs[i].ticks.is_multiple_of(20) {
            return;
        }
        let area = self.orbs[i].bounds().inflate(Vec3::new(0.5, 0.5, 0.5));
        let mut absorbed = Vec::new();
        for j in 0..self.orbs.len() {
            if j == i || !self.orbs[j].bounds().intersects(area) {
                continue;
            }
            let (vanilla, into, into_value, from, from_value, count, age) = {
                let (a, b) = (&self.orbs[i], &self.orbs[j]);
                (
                    b.can_merge(a.id, a.value),
                    a.id,
                    a.points(),
                    b.id,
                    b.points(),
                    b.count,
                    b.age,
                )
            };
            if !self.orb_merges(into, into_value, from, from_value, vanilla) {
                continue;
            }
            self.orbs[i].count = self.orbs[i].count.saturating_add(count);
            self.orbs[i].age = self.orbs[i].age.min(age);
            absorbed.push(j);
        }
        for j in absorbed.into_iter().rev() {
            self.orbs.remove(j);
        }
    }
    /// `Entity.unstuckIfPossible`, which `ExperienceOrb`'s constructor runs on an
    /// orb whose box is in a block: the free spot nearest its box's centre. An
    /// orb in open air keeps its position.
    pub(super) fn unstuck_orb(&self, at: [f64; 3]) -> [f64; 3] {
        let b = self.res.behaviours();
        let border = self.border_bounds();
        ExperienceOrb::unstuck(Vec3::new(at[0], at[1], at[2]), &border, |p| {
            b.collision_shape(self.world.block(p.x, p.y, p.z))
                .map(|s| s.moved(Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z))))
        })
        .map_or(at, |p| [p.x, p.y, p.z])
    }
    /// `new ExperienceOrb(level, pos, Vec3.ZERO, value)`: yaw and motion from
    /// the orb's own random.
    pub(super) fn add_orb(&mut self, at: [f64; 3], value: u32) {
        self.add_orb_toward(at, [0.0; 3], value);
    }
    /// `new ExperienceOrb(level, pos, roughly, value)`: a random motion that
    /// points against `roughly` is turned round, and the orb starts half its
    /// size (0.25) along `roughly`, unstuck if it collides there.
    pub(super) fn add_orb_toward(&mut self, at: [f64; 3], roughly: [f64; 3], value: u32) {
        let len_sqr = roughly.iter().map(|v| v * v).sum::<f64>();
        // Vec3.normalize: a vector shorter than 1e-5 has no direction.
        let len = len_sqr.sqrt();
        let unit = if len < 1.0e-5 {
            [0.0; 3]
        } else {
            roughly.map(|v| v / len)
        };
        let at = self.unstuck_orb([0, 1, 2].map(|a| at[a] + unit[a] * 0.25));
        let mut r = random_source::entity();
        let _yaw = r.next_float();
        let p = Vec3::new(at[0], at[1], at[2]);
        let mut velocity = Vec3::new(
            (r.next_double() * 0.2 - 0.1) * 2.0,
            r.next_double() * 0.2 * 2.0,
            (r.next_double() * 0.2 - 0.1) * 2.0,
        );
        let dot = roughly[0] * velocity.x + roughly[1] * velocity.y + roughly[2] * velocity.z;
        if len_sqr > 0.0 && dot < 0.0 {
            velocity = Vec3::new(-velocity.x, -velocity.y, -velocity.z);
        }
        self.next_entity += 1;
        self.orbs.push(ExperienceOrb {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            id: self.next_entity,
            position: p,
            previous: p,
            velocity,
            // `getExperienceValue` sizes: 1 to 2477.
            value: value as i32,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
            health: 5,
            following: None,
        });
        // ServerLevel.addFreshEntity.
        self.native_joins_flush(false, 0);
    }
    /// The orbs' tick, but for the movement: that is `tick_ordered_before`'s,
    /// in join order with the other kinds.
    pub(super) fn tick_experience(&mut self) {
        if self.xp_delay > 0 {
            self.xp_delay -= 1;
        }
        let spectator = self.spectator();
        let player = self
            .player
            .as_ref()
            .filter(|p| !p.health.dead_or_dying() && !spectator);
        // Player.aiStep: standing, the pickup area is the player's box
        // inflated by (1, 0.5, 1); riding, the union with the vehicle's box
        // inflated by (1, 0, 1).
        let bounds = player.map(|p| {
            let w = f64::from(p.dimensions.width) / 2.0;
            let own = Aabb::new(
                Vec3::new(p.position.x - w, p.position.y, p.position.z - w),
                Vec3::new(
                    p.position.x + w,
                    p.position.y + f64::from(p.dimensions.height),
                    p.position.z + w,
                ),
            );
            let area = match self.riding_box() {
                Some(vehicle) => own.union_bounds(vehicle).inflate(Vec3::new(1.0, 0.0, 1.0)),
                None => own.inflate(Vec3::new(1.0, 0.5, 1.0)),
            };
            (
                [
                    area.min_position().x,
                    area.min_position().y,
                    area.min_position().z,
                ],
                [
                    area.max_position().x,
                    area.max_position().y,
                    area.max_position().z,
                ],
            )
        });
        let mut orbs = std::mem::take(&mut self.orbs);
        let mut gained = 0;
        let mut taken = Vec::new();
        if let Some((min, max)) = bounds {
            orbs.retain_mut(|o| {
                let touching = o.position.x + 0.25 > min[0]
                    && o.position.x - 0.25 < max[0]
                    && o.position.y + 0.5 > min[1]
                    && o.position.y < max[1]
                    && o.position.z + 0.25 > min[2]
                    && o.position.z - 0.25 < max[2];
                if touching && self.xp_delay == 0 {
                    // ExperienceOrb.playerTouch: one orb every 2 ticks; a
                    // merged orb gives its value once per count.
                    self.xp_delay = 2;
                    gained += o.points();
                    taken.push(o.clone());
                    o.count -= 1;
                    return o.count > 0;
                }
                true
            });
        }
        self.orbs = orbs;
        if gained > 0 {
            gained = self.mending(gained);
        }
        for o in taken {
            // ClientPacketListener.handleTakeItemEntity: every orb taken sounds
            // at the orb, whether mending took its points or not.
            let pitch =
                (self.world.random.next_float() - self.world.random.next_float()) * 0.35 + 0.9;
            let at = [o.position.x, o.position.y, o.position.z];
            self.audio.play(
                "minecraft:entity.experience_orb.pickup",
                Some(at),
                0.1,
                pitch,
            );
            self.add_pickup(pickups::Taken::Orb(Box::new(o)), pickups::Collector::Player);
        }
        if gained > 0 {
            self.give_orb_points(gained);
        }
    }
}

#[cfg(test)]
mod escape_tests {
    use super::super::native_machines::tests::server_game;
    use horizon_client::chunk::Chunk;
    use horizon_client::items::ItemStack;
    use horizon_core::random::RandomSource;
    use std::sync::Arc;

    /// `Entity.moveTowardsClosestSpace` takes an item's escape speed from the
    /// item's own random, the one its constructor drew from, and leaves the
    /// level's random as it was (#13043).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn an_item_escapes_a_block_at_its_own_randoms_speed() {
        let mut game = server_game();
        let feet = horizon_core::geometry::Vec3::new(8.5, 70.0, 8.5);
        game.player = Some(horizon_client::motion::PlayerMotion::new(game.motion, feet).unwrap());
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.update_chunk_tickets();
        assert!(game.entity_ticking_area()(8, 8));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.set(8, 64, 8, stone).unwrap();
        let stack = ItemStack::new("minecraft:stone", 2);
        game.spawn_item(stack, [8.5, 64.4, 8.5], [0.0, 0.0, 0.0], 10);
        let item = game.dropped.last().unwrap();
        let expected = f64::from(item.random.clone().next_float() * 0.2 + 0.1);
        let mut level = game.world.random.clone();
        game.ticked_entities_through = u32::MAX;
        game.tick_items();
        let item = game.dropped.last().unwrap();
        // Up is the nearest open side from the middle of the block.
        // The push replaces the fall's velocity; then the 0.98 drag.
        assert_eq!(item.velocity.y, expected * 0.98);
        assert_eq!(game.world.random.next_float(), level.next_float());
    }
}
