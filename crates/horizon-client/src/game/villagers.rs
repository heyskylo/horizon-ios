//! What villagers do with each other and with the player: gossip and
//! reputation, sharing food and wares, picking up dropped items, making
//! babies, calling iron golems and the prices they ask.
use super::Game;
use horizon_client::{
    gossip::{self, GossipType},
    items::ItemStack,
    mobs::{Mob, Species, spawning, villager, villager_brain},
};
use horizon_core::geometry::Vec3;

fn block_of(p: Vec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}
impl Game<'_> {
    fn villager_index(&self, id: u32) -> Option<usize> {
        self.mobs
            .iter()
            .position(|m| m.id == id && m.species() == Species::Villager)
    }
    fn item_max_stack(&self, item: &str) -> u32 {
        self.items().get(item).map_or(64, |i| i.max_stack)
    }
    /// `WorkAtComposter.useWorkstation`: at a composter the farmer makes
    /// bread, then composts seeds.
    pub(super) fn villager_work_at_composter(&mut self, id: u32, pos: [i32; 3]) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        if self.composter_level(pos).is_none() {
            return;
        }
        self.villager_make_bread(i);
        self.villager_compost_items(i, pos);
    }
    /// `WorkAtComposter.makeBread`: up to three loaves from nine wheat while
    /// the villager holds at most 36 bread; what does not fit drops.
    fn villager_make_bread(&mut self, i: usize) {
        let max_stack = self.item_max_stack("minecraft:bread");
        let e = villager::ext_mut(&mut self.mobs[i]);
        if e.count_item("minecraft:bread") > 36 {
            return;
        }
        let loaves = (e.count_item("minecraft:wheat") / 3).min(3);
        if loaves == 0 {
            return;
        }
        e.remove_item("minecraft:wheat", loaves * 3);
        let left = e.add_item(ItemStack::new("minecraft:bread", loaves), max_stack);
        if let Some(rest) = left {
            // spawnAtLocation(level, stack, 0.5F)
            let p = self.mobs[i].position;
            self.spawn_item(rest, [p.x, p.y + 0.5, p.z], [0.0, 0.2, 0.0], 10);
        }
    }
    /// `BehaviorUtils.throwItem`: toss a stack from a villager's hand toward
    /// a point.
    fn villager_throw(&mut self, from: Vec3, eye: f64, to: Vec3, stack: ItemStack) {
        // The hand drop and the velocity are the float 0.3F, widened.
        let k = f64::from(0.3_f32);
        let at = [from.x, eye - k, from.z];
        let d = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z).normalize();
        self.spawn_item(stack, at, [d.x * k, d.y * k, d.z * k], 10);
    }

    /// `TradeWithVillager.tick` for one pair: gossip (at most once in 1200
    /// ticks for both), then surplus food and wanted wares tossed over.
    pub(super) fn villager_meet(&mut self, a: u32, b: u32) {
        let (Some(ia), Some(ib)) = (self.villager_index(a), self.villager_index(b)) else {
            return;
        };
        let now = self.world.game_time;
        let items = world_items!(self);
        let meeting = villager::meet(&mut self.mobs, ia, ib, now, |item| {
            items.get(item).map_or(64, |i| i.max_stack)
        });
        for stack in meeting.thrown {
            self.villager_throw(meeting.from, meeting.eye, meeting.to, stack);
        }
        if meeting.gossiped {
            self.villager_golem_check(a, 5);
        }
    }

    /// `Villager.spawnGolemIfNeeded`: when enough villagers nearby that
    /// slept recently and saw no golem agree, an iron golem appears.
    pub(super) fn villager_golem_check(&mut self, id: u32, needed: usize) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        let now = self.world.game_time;
        let Some(near) = villager::golem_summoners(&self.mobs, i, now, needed) else {
            return;
        };
        if self.try_spawn_iron_golem(block_of(self.mobs[i].position)) {
            for j in near {
                villager_brain::golem_detected(&mut self.mobs[j], now);
            }
        }
    }
    /// `SpawnUtil.trySpawnMob(IRON_GOLEM, ...)`: a golem near `start`, with
    /// the ambient sound `trySpawnMob` plays.
    fn try_spawn_iron_golem(&mut self, start: [i32; 3]) -> bool {
        let Some(kind) = self.res.mob_types().get("minecraft:iron_golem").cloned() else {
            return false;
        };
        let id = self.next_entity + 1;
        let mut random = self.world.random.clone();
        let golem = villager::summon_golem(
            &self.mob_world_base(&[]),
            start,
            &mut random,
            |random, at, yaw| {
                let mut m = Mob::new(id, kind.clone(), at, yaw);
                villager::finalize_summoned_golem(&mut m, random);
                m
            },
        );
        self.world.random = random;
        let Some(golem) = golem else {
            return false;
        };
        self.next_entity = id;
        let sound = golem.kind.sounds[0];
        if !sound.is_empty() {
            let at = golem.center();
            self.audio
                .play(&format!("minecraft:{sound}"), Some(at), 1.0, 1.0);
        }
        self.mobs.push(golem);
        self.native_mobs_joined(self.mobs.len() - 1, false);
        true
    }

    /// `VillagerMakeLove.tryToGiveBirth`: both eat, a free bed within 48
    /// blocks is taken for the child, who is a baby villager of one of
    /// three villager types.
    pub(super) fn villager_birth(
        &mut self,
        parent: u32,
        partner: u32,
        child: Option<([i32; 3], Option<u8>)>,
    ) {
        let (Some(ia), Some(ib)) = (self.villager_index(parent), self.villager_index(partner))
        else {
            return;
        };
        villager::ext_mut(&mut self.mobs[ib]).eat_and_digest();
        let from = self.mobs[ia].position;
        let Some((bed, variant)) = child else {
            // VillagerMakeLove: entity event 13 for the partner, then the parent.
            for i in [ib, ia] {
                let id = self.mobs[i].id;
                self.mob_event_particles(id, 13);
            }
            return;
        };
        let variant = variant.unwrap_or_else(|| {
            let facts = self.biome_facts(
                from.x.floor() as i32,
                from.y.floor() as i32,
                from.z.floor() as i32,
            );
            spawning::villager_type(&facts)
        });
        self.mobs[ib].extra.age = 6000;
        let Some(id) = self.spawn_mob_raw("minecraft:villager", [from.x, from.y, from.z], true)
        else {
            return;
        };
        if let Some(child) = self.mobs.iter_mut().find(|m| m.id == id) {
            child.extra.variant = variant;
            child.extra.variant2 = villager::NONE;
            child.extra.age = -24000;
            child.extra.bed = Some(bed);
        }
        // VillagerMakeLove.giveBirth: entity event 12 for the child.
        self.mob_event_particles(id, 12);
    }
    /// `Level.broadcastEntityEvent` of an event whose `handleEntityEvent`
    /// draws particles about the mob (villager 12, 13 and 14, taming 6 and 7,
    /// love 18): the guests get the byte, and this game draws it as its own
    /// client would.
    pub(super) fn mob_event_particles(&mut self, id: u32, event: i8) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let at = [m.position.x, m.position.y, m.position.z];
        self.lan_entity_event(id as i32, event, at);
        self.entity_event_particles(id, event);
    }
    /// `handleEntityEvent(event)`'s particles about mob `id`
    /// (`Villager`, `TamableAnimal`, `AbstractHorse`, `Animal`, `Allay`).
    pub(super) fn entity_event_particles(&mut self, id: u32, event: i8) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        if let Some((kind, burst)) =
            horizon_client::particles::entity_event_burst(&m.kind.id, event as u8)
        {
            self.particles_around_mob(id, kind, burst);
        }
    }
    /// `particles::around_self` about mob `id`.
    pub(super) fn particles_around_mob(
        &mut self,
        id: u32,
        kind: &'static str,
        burst: horizon_client::particles::Burst,
    ) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let feet = [m.position.x, m.position.y, m.position.z];
        let size = [f64::from(m.width()), f64::from(m.height())];
        self.particles_around(kind, feet, size, burst);
    }
    /// `particles::around_self` on the client: the particle engine's random
    /// stands in for the client entity's, so no server draw moves.
    pub(super) fn particles_around(
        &mut self,
        kind: &'static str,
        feet: [f64; 3],
        size: [f64; 2],
        burst: horizon_client::particles::Burst,
    ) {
        let Some(ty) = horizon_client::particles::ParticleType::from_name(kind) else {
            return;
        };
        let options = horizon_client::particles::ParticleOptions::simple(ty);
        let mut spawned = Vec::new();
        horizon_client::particles::around_self(
            self.particles.random(),
            feet,
            size[0],
            size[1],
            burst,
            |p, v| spawned.push((p, v)),
        );
        for (p, v) in spawned {
            self.particles.add(&options, p, v);
        }
    }

    /// `NearestItemSensor.doTick` for the villagers whose sensor is due: the
    /// nearest dropped item within 32 blocks that `Villager.wantsToPickUp`
    /// accepts and the villager sees, or none.
    pub(super) fn villager_item_sense(&mut self) {
        if self.dropped.is_empty() {
            for m in &mut self.mobs {
                if m.species() == Species::Villager
                    && self.creatures.ticked.contains(&m.id)
                    && villager_brain::take_item_scan(m)
                {
                    villager_brain::set_wanted_item(m, None);
                }
            }
            return;
        }
        let picks_up = self
            .res
            .items()
            .item_tags
            .get("minecraft:villager_picks_up")
            .cloned();
        let mut due = Vec::new();
        for (i, m) in self.mobs.iter_mut().enumerate() {
            if m.species() == Species::Villager
                && self.creatures.ticked.contains(&m.id)
                && villager_brain::take_item_scan(m)
            {
                due.push(i);
            }
        }
        if due.is_empty() {
            return;
        }
        let w = self.mob_world_base(&[]);
        let mut found = Vec::new();
        for i in due {
            let m = &self.mobs[i];
            let area = m.bounds().inflate(Vec3::new(32.0, 16.0, 32.0));
            let requested = villager::requested_items(m);
            let eye = Vec3::new(m.position.x, m.eye_y(), m.position.z);
            let mut best: Option<(f64, Vec3)> = None;
            for e in &self.dropped {
                let d = e.position.distance_squared(m.position);
                if best.is_some_and(|(b, _)| d >= b)
                    || !e.bounds().intersects(area)
                    || d >= 32.0 * 32.0
                {
                    continue;
                }
                let item = &e.stack.item;
                let wants = villager::food_points(item) > 0
                    || picks_up.as_ref().is_some_and(|t| t.contains(&**item))
                    || requested.contains(&&**item);
                let fits =
                    villager::ext(m).is_none_or(|x| x.can_add(&e.stack, self.item_max_stack(item)));
                // ItemEntity's eye height is 0.2125.
                let at = Vec3::new(e.position.x, e.position.y + 0.2125, e.position.z);
                if wants && fits && w.line_of_sight(eye, at) {
                    best = Some((d, e.position));
                }
            }
            found.push((i, best.map(|(_, p)| [p.x, p.y, p.z])));
        }
        for (i, at) in found {
            villager_brain::set_wanted_item(&mut self.mobs[i], at);
        }
    }
    /// `Mob.aiStep` item pickup for villagers (`InventoryCarrier`): food,
    /// seeds and what their profession wants, from within a block.
    pub(super) fn villager_pickups(&mut self) {
        if self.dropped.is_empty() || !self.ui.rules.bool("mob_griefing") {
            return;
        }
        let picks_up = self
            .res
            .items()
            .item_tags
            .get("minecraft:villager_picks_up")
            .cloned();
        // A villager with no item near skips its scan; one near an item scans
        // the items of the cells it meets as before, testing the cheap reach
        // first: a server's villagers and dropped items each run into the
        // hundreds or thousands.
        let mut near_items = super::item_areas::ItemAreas::default();
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            if m.species() != Species::Villager
                || !m.alive()
                || !self.creatures.ticked.contains(&m.id)
            {
                continue;
            }
            let (pos, half) = (m.position, f64::from(m.width()) / 2.0 + 1.0);
            let top = f64::from(m.height());
            let reach = horizon_core::geometry::Aabb::new(
                Vec3::new(pos.x - half, pos.y - 0.25, pos.z - half),
                Vec3::new(pos.x + half, pos.y + top, pos.z + half),
            );
            let mut scan = near_items.scan(&self.dropped, self.next_entity, reach);
            if scan.is_empty() {
                continue;
            }
            let requested = villager::requested_items(m);
            let mut j = 0;
            loop {
                j = scan.next_from(j, self.dropped.len());
                let Some(e) = self.dropped.get(j) else {
                    break;
                };
                let near = (e.position.x - pos.x).abs() <= half
                    && (e.position.z - pos.z).abs() <= half
                    && e.position.y >= pos.y - 0.25
                    && e.position.y <= pos.y + top;
                if !near || e.pickup_delay > 0 {
                    j += 1;
                    continue;
                }
                let item = e.stack.item.clone();
                let wants = villager::food_points(&item) > 0
                    || picks_up.as_ref().is_some_and(|t| t.contains(&*item))
                    || requested.contains(&&*item);
                if !wants {
                    j += 1;
                    continue;
                }
                let max = self.item_max_stack(&item);
                let stack = self.dropped[j].stack.clone();
                if !villager::ext_mut(&mut self.mobs[i]).can_add(&stack, max) {
                    j += 1;
                    continue;
                }
                let before = self.dropped[j].clone();
                let rest = villager::ext_mut(&mut self.mobs[i]).add_item(stack, max);
                if rest.as_ref().is_none_or(|r| r.count < before.stack.count) {
                    let id = self.mobs[i].id;
                    self.add_pickup(
                        super::pickups::Taken::Item(Box::new(before)),
                        super::pickups::Collector::Mob(id),
                    );
                }
                match rest {
                    None => {
                        self.remove_dropped(j);
                        scan.removed(j);
                    }
                    Some(rest) => {
                        self.dropped[j].stack.count = rest.count;
                        j += 1;
                    }
                }
            }
        }
    }

    /// The UUID of the player who last hurt a mob (`last_hurt_by_player`:
    /// None for the host, else the guest's connection): the reputation a
    /// villager keeps is that player's, the one trading looks up.
    pub(super) fn player_uuid_of(&self, who: Option<u32>) -> Option<[i32; 4]> {
        let uuid = match who {
            None => return self.level.player_uuid,
            // While that guest acts its own UUID is swapped in.
            Some(conn) if self.lan_acting == Some(conn) => self.lan_acting_uuid?,
            Some(conn) => self.lan.as_ref()?.hub.guest(conn)?.uuid,
        };
        Some(horizon_client::lan::playerdata::uuid_ints(uuid))
    }
    /// Capture a causing entity's identity before its damage is deferred.
    pub(super) fn damage_attacker_uuid(
        &mut self,
        who: Option<horizon_client::mobs::Who>,
        guest: Option<u32>,
    ) -> Option<[i32; 4]> {
        match who {
            Some(horizon_client::mobs::Who::Player) => self.player_uuid_of(guest),
            Some(horizon_client::mobs::Who::Mob(id)) => {
                if let Some(uuid) = self.persist.uuids.get(&id) {
                    Some(*uuid)
                } else if self.mobs.iter().any(|m| m.id == id && !m.removed) {
                    Some(self.mob_uuid(id))
                } else {
                    None
                }
            }
            None => None,
        }
    }
    /// Flush only notifications for this merchant. The causing UUID was
    /// captured with the accepted hit; an actor swap never reconstructs it.
    pub(super) fn flush_villager_reputation(&mut self, id: u32) {
        if !self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(villager::ext)
            .is_some_and(|e| !e.reputation_events.is_empty())
        {
            return;
        }
        if self.lan_acting.is_some() {
            if !self.pending_villager_reputation.contains(&id) {
                self.pending_villager_reputation.push(id);
            }
            return;
        }
        let changes = self.take_villager_reputation_events(id);
        for uuid in changes {
            self.merchant_refresh_local(id, Some(uuid));
            if let Some(mut host) = self.lan.take() {
                self.lan_merchant_refresh(&mut host, id, Some(uuid));
                self.lan = Some(host);
            }
        }
    }
    fn take_villager_reputation_events(&mut self, id: u32) -> Vec<[i32; 4]> {
        self.mobs
            .iter_mut()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.villager.as_deref_mut())
            .map(|e| std::mem::take(&mut e.reputation_events))
            .unwrap_or_default()
    }
    /// Run after the guest's inventory, effects and merchant record are back
    /// in the hub, so refreshes see their actual current trading session.
    pub(super) fn flush_guest_villager_reputation(&mut self, host: &mut super::lan::LanHost) {
        // A nested return restores an outer borrowed player, whose merchant
        // and effects are still outside the hub. Wait for its canonical restore.
        if self.lan_acting.is_some() {
            return;
        }
        for id in std::mem::take(&mut self.pending_villager_reputation) {
            for uuid in self.take_villager_reputation_events(id) {
                self.merchant_refresh_local(id, Some(uuid));
                self.lan_merchant_refresh(host, id, Some(uuid));
            }
        }
    }
    /// Deliver accepted mob hurts whose UUID could not be captured in the
    /// low-level damage pipeline. Player identities are never reconstructed here.
    pub(super) fn villager_reputation_tick(&mut self) {
        let mut pending = Vec::new();
        for m in &mut self.mobs {
            if let Some(e) = m.extra.villager.as_deref_mut() {
                for who in std::mem::take(&mut e.hurts) {
                    pending.push((m.id, who));
                }
            }
        }
        for (id, who) in pending {
            let horizon_client::mobs::Who::Mob(attacker) = who else {
                continue;
            };
            let Some(uuid) =
                self.damage_attacker_uuid(Some(horizon_client::mobs::Who::Mob(attacker)), None)
            else {
                continue;
            };
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                villager::reputation_event(m, uuid, GossipType::MinorNegative);
            }
        }
        let changed: Vec<_> = self
            .mobs
            .iter()
            .filter(|m| villager::ext(m).is_some_and(|e| !e.reputation_events.is_empty()))
            .map(|m| m.id)
            .collect();
        for id in changed {
            self.flush_villager_reputation(id);
        }
    }
    /// `Villager.tellWitnessesThatIWasMurdered`: cached witnesses remember
    /// the actual causing entity, including a mob.
    pub(super) fn villager_murdered(&mut self, victim: &Mob) {
        if victim.species() != Species::Villager {
            return;
        }
        let Some(attacker) = victim.last_damage.and_then(|(source, _)| source.attacker) else {
            return;
        };
        let uuid = match victim
            .last_damage
            .and_then(|(source, _)| source.attacker_uuid)
        {
            Some(uuid) => uuid,
            None => match attacker {
                horizon_client::mobs::Who::Mob(id) => {
                    let Some(uuid) =
                        self.damage_attacker_uuid(Some(horizon_client::mobs::Who::Mob(id)), None)
                    else {
                        return;
                    };
                    uuid
                }
                horizon_client::mobs::Who::Player => return,
            },
        };
        let snapshot: Vec<_> = self.mobs.iter().map(|m| m.seen(None)).collect();
        let witnesses =
            villager_brain::murder_witnesses(&mut victim.clone(), &self.mob_world_base(&snapshot));
        for id in witnesses {
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                villager::reputation_event(m, uuid, GossipType::MajorNegative);
            }
            self.flush_villager_reputation(id);
        }
    }
    /// Retain the accepted trade's actual player until the eligible mob AI
    /// phase, including across menu close. XP and rewards stay in the session.
    pub(super) fn villager_trade_pending(&mut self, id: u32) {
        let Some(player) = self.player_uuid_of(self.lan_acting) else {
            return;
        };
        let Some(i) = self.villager_index(id) else {
            return;
        };
        villager::ext_mut(&mut self.mobs[i]).last_traded_player = Some(player);
    }
    /// The TRADE reputation event from the player `uuid` (a guest's too).
    pub(super) fn villager_traded_by(&mut self, id: u32, uuid: [i32; 4]) {
        let Some(i) = self.villager_index(id) else {
            return;
        };
        villager::ext_mut(&mut self.mobs[i]).gossips.add(
            uuid,
            GossipType::Trading,
            gossip::REPUTATION_CHANGE_PER_TRADE,
        );
        // Villager.customServerAiStep after a trade: entity event 14.
        self.mob_event_particles(id, 14);
        // onReputationEventFrom refreshes the current trading player's prices.
        if self
            .station
            .as_ref()
            .and_then(|s| s.merchant.as_ref())
            .is_some_and(|m| m.mob == Some(id))
        {
            let reputation = self.villager_reputation(id);
            self.menu_session(|menu, env, _| {
                if let Some(session) = menu.merchant.as_mut() {
                    session.reputation = reputation;
                    session.update_special_prices();
                }
                menu.merchant_update(env);
                Some(true)
            });
        }
        if let Some(mut host) = self.lan.take() {
            self.lan_merchant_reputation(&mut host, id);
            self.lan = Some(host);
        }
    }
    /// `Villager.getPlayerReputation` of the local player.
    pub(super) fn villager_reputation(&self, id: u32) -> i32 {
        let Some(uuid) = self.level.player_uuid else {
            return 0;
        };
        self.villager_reputation_of(id, uuid)
    }
    /// `Villager.getPlayerReputation` of the player `uuid`.
    pub(super) fn villager_reputation_of(&self, id: u32, uuid: [i32; 4]) -> i32 {
        self.villager_index(id)
            .and_then(|i| villager::ext(&self.mobs[i]))
            .map_or(0, |e| e.gossips.reputation(uuid))
    }
    /// The Hero of the Village amplifier of the player, if any.
    pub(super) fn hero_amplifier(&self) -> Option<i32> {
        self.potions.effects.amplifier("hero_of_the_village")
    }
    /// `AbstractBedBlock.kickVillagerOutOfBed`: the first sleeping villager
    /// whose box touches the bed's head block wakes up
    /// (`Villager.stopSleeping`). Whether one did.
    pub(super) fn kick_villager_out_of_bed(&mut self, head: [i32; 3]) -> bool {
        let min = Vec3::new(f64::from(head[0]), f64::from(head[1]), f64::from(head[2]));
        let max = Vec3::new(min.x + 1.0, min.y + 1.0, min.z + 1.0);
        let Some(i) = self.mobs.iter().position(|m| {
            m.species() == Species::Villager
                && villager::sleeping(m)
                && m.bounds().intersects_corners(min, max)
        }) else {
            return false;
        };
        let id = self.mobs[i].id;
        self.wake_villager(id);
        true
    }
    /// `LivingEntity.stopSleeping` for the villager `id`, outside its own
    /// tick: it stands up and its bed is no longer occupied.
    pub(super) fn wake_villager(&mut self, id: u32) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        let mut mobs = std::mem::take(&mut self.mobs);
        let mut events = Vec::new();
        villager::stop_sleeping(&mut mobs[i], &self.mob_world_base(&[]), &mut events);
        villager_brain::woken(&mut mobs[i], self.world.game_time);
        self.mobs = mobs;
        if let Err(e) = self.handle_mob_events(events) {
            log::warn!("waking villager {id}: {e}");
        }
    }
}

#[cfg(test)]
mod entity_event_particle_tests {
    /// The raid sweat (42) and the dolphin's treasure hearts (38) are in the
    /// table that now decides which entity events draw particles.
    #[test]
    fn the_table_draws_the_raid_sweat_and_the_dolphin_event() {
        let kind = |entity, event| {
            horizon_client::particles::entity_event_burst(entity, event).map(|(k, _)| k)
        };
        assert_eq!(kind("villager", 42), Some("splash"));
        assert_eq!(kind("dolphin", 38), Some("happy_villager"));
        assert_eq!(kind("villager", 2), None);
    }
}

#[cfg(test)]
#[path = "villager_reputation_tests.rs"]
mod reputation_tests;
