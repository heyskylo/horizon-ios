//! The mob loop of `tick_creature_entities` over a group of mobs, in their list order, with
//! everything the loop keeps for them: their snapshot and its indexes, the pushes and events
//! their ticks give, the level random they draw from, the players they see and what dies. The
//! serial loop is one group holding every mob; a group of the parallel mob tick (#14457) holds
//! the mobs no mob outside it can reach this tick.
use super::*;
use horizon_client::{
    fast_hash::{FxHashMap, FxHashSet},
    mobs::{MobSlot, PendingPushes, seen_grid::SeenGrid, spawning::LocalDifficulty},
};
use horizon_core::random::LegacyRandom;

/// What every mob of the loop reads and none writes.
pub(super) struct Shared<'a> {
    pub uuid: Option<[i32; 4]>,
    pub level_id: &'static str,
    /// The blocks whose collision changed since the last tick: mobs re-path around them.
    pub path_changes: &'a [[i32; 3]],
    /// The guests' entity ids by connection, for the shulker bullets fired at the guest a mob
    /// sees.
    pub guest_ids: &'a [(u32, u32)],
}

/// A mob whose body goes this tick: (id, feet, width, height, the grown cube it splits into).
/// The last field is the grown slime, magma cube or sulfur cube the body leaves behind
/// (`AbstractCubeMob.remove`), captured as the body goes because it has left the list by the
/// time it splits.
pub(super) type Gone = (u32, Vec3, f64, f64, Option<Mob>);

/// A group's mobs and everything the loop keeps for them, in their list order.
pub(super) struct Group<M: MobSlot> {
    pub mobs: Vec<M>,
    /// Each mob's place in the whole list; empty when the group is the whole list.
    pub global: Vec<usize>,
    /// After each turn, where the turn's entries end: in `events`, `dead`, `corpses`, `gone` and
    /// `guest_events`. A turn's entries stay where it left them.
    pub ends: Vec<[usize; 5]>,
    /// Each mob's snapshot entry: as it was before the tick until its turn, as its turn left it
    /// after.
    pub snapshot: Vec<Seen>,
    grid: SeenGrid,
    /// The first slot of each id, as a scan would find it.
    index: FxHashMap<u32, u32>,
    villager_slots: Vec<u32>,
    solid_slots: Vec<u32>,
    pub pushes: PendingPushes,
    pub events: Vec<MobEvent>,
    /// The level random, lent to the mobs (brain behaviours draw from it).
    pub random: LegacyRandom,
    pub guests: super::super::lan_mobs::GuestViews,
    /// What stands in a guest's slot while a mob holds that guest's view for its tick.
    spare: Option<PlayerView>,
    /// The host's own player, lent to each mob that sees it for its tick.
    pub player: Option<PlayerView>,
    /// The guest each mob sees, by mob id.
    pub bindings: FxHashMap<u32, u32>,
    pub died: FxHashSet<u32>,
    pub ticked: FxHashSet<u32>,
    /// Each mob's target as the mods last heard it (`native_living_change_target`).
    pub targets: Vec<Option<Who>>,
    /// Whether each mob stays in the list, pushed at its turn.
    pub keep: Vec<bool>,
    pub dead: Vec<Mob>,
    pub corpses: Vec<Mob>,
    pub gone: Vec<Gone>,
    pub guest_events: Vec<(u32, u32, MobEvent)>,
}

/// What decides each turn besides the group, read at the turn as the serial loop reads it.
pub(super) struct Turns<'t, 'w> {
    pub base: &'t MobWorld<'w>,
    /// `base` for the despawn checks: no snapshot, no player.
    pub despawn: MobWorld<'w>,
    /// Whether a block column is in an entity-ticking chunk.
    pub ticking: &'t dyn Fn(i32, i32) -> bool,
    pub nearest: &'t super::prep::MobsNearest<'t>,
    /// The one player's position, when the level has one: every mob's nearest.
    pub single_player: Option<Option<Vec3>>,
    /// Whether a mod canceled a mob's tick.
    pub canceled: &'t dyn Fn(u32) -> bool,
    /// `getCurrentDifficultyAt` a position.
    pub difficulty: &'t (dyn Fn([f64; 3]) -> LocalDifficulty + Sync),
    /// The last chunk's difficulty: mobs come in runs of one chunk.
    pub cached_difficulty: Option<((i32, i32), LocalDifficulty)>,
}

/// What a group starts from besides its mobs and their snapshot.
#[derive(Clone)]
pub(super) struct Start {
    pub events: Vec<MobEvent>,
    pub random: LegacyRandom,
    pub guests: super::super::lan_mobs::GuestViews,
    pub player: Option<PlayerView>,
    pub bindings: FxHashMap<u32, u32>,
    pub died: FxHashSet<u32>,
    pub ticked: FxHashSet<u32>,
    pub targets: Vec<Option<Who>>,
}

impl<M: MobSlot> Group<M> {
    /// A group of `mobs`, whose snapshot entries are `snapshot`.
    pub(super) fn new(mobs: Vec<M>, snapshot: Vec<Seen>, start: Start) -> Self {
        let grid = SeenGrid::new(snapshot.iter().map(|s| s.bounds));
        let mut index = FxHashMap::with_capacity_and_hasher(snapshot.len(), Default::default());
        let (mut villager_slots, mut solid_slots) = (Vec::new(), Vec::new());
        for (i, s) in snapshot.iter().enumerate() {
            index.entry(s.id).or_insert(i as u32);
            if s.species == Species::Villager {
                villager_slots.push(i as u32);
            }
            if s.may_block_mobs() {
                solid_slots.push(i as u32);
            }
        }
        let n = mobs.len();
        Self {
            spare: start.guests.first().map(|g| g.1.clone()),
            mobs,
            global: Vec::new(),
            ends: Vec::with_capacity(n),
            snapshot,
            grid,
            index,
            villager_slots,
            solid_slots,
            pushes: PendingPushes::default(),
            events: start.events,
            random: start.random,
            guests: start.guests,
            player: start.player,
            bindings: start.bindings,
            died: start.died,
            ticked: start.ticked,
            targets: start.targets,
            keep: Vec::with_capacity(n),
            dead: Vec::new(),
            corpses: Vec::new(),
            gone: Vec::new(),
            guest_events: Vec::new(),
        }
    }

    /// The group's spatial query extent and scan threshold match the whole list's `whole`.
    pub(super) fn answering_as(self, whole: (f64, usize)) -> Self {
        Self {
            grid: self.grid.answering_as(whole),
            ..self
        }
    }

    /// Mob `i`'s place in the whole list.
    pub(super) fn at(&self, i: usize) -> usize {
        self.global.get(i).copied().unwrap_or(i)
    }

    /// Every mob's turn, in order. A group of the parallel tick draws the level random in list
    /// order with the other groups (`OrderedDraws`), and shows each mob after its turn to the
    /// mobs of the other groups that look it up by id (`foreign`).
    pub(super) fn run(
        &mut self,
        turns: &mut Turns,
        shared: &Shared,
        foreign: Option<&super::mob_islands::Foreign>,
    ) {
        // A panic in a turn still lets the other groups finish (the threads' join passes it on).
        let _done = self.random.lane_done();
        for i in 0..self.mobs.len() {
            let at = self.at(i);
            self.random.begin_item(at);
            if let Some(foreign) = foreign {
                foreign.reader.set(at);
            }
            if let Some(foreign) = foreign {
                foreign.in_seed_order(at, || self.turn(i, turns, shared));
            } else {
                self.turn(i, turns, shared);
            }
            let ends = [
                self.events.len(),
                self.dead.len(),
                self.corpses.len(),
                self.gone.len(),
                self.guest_events.len(),
            ];
            self.ends.push(ends);
            if let Some(foreign) = foreign {
                foreign.publish(at, &self.snapshot[i]);
            }
            self.random.end_item();
        }
        self.random.finish_lane();
    }

    /// Mob `i`'s turn: whether its chunk is loaded and ticking is decided now, as the serial
    /// loop decides it, then its tick or its despawn check.
    fn turn(&mut self, i: usize, turns: &mut Turns, shared: &Shared) {
        let position = self.mobs[i].mob().position;
        let (bx, bz) = (position.x.floor() as i32, position.z.floor() as i32);
        let seen_guest = self.bind_guest(i);
        if !turns.base.world.loaded(bx, bz) {
            self.unloaded(i);
            return;
        }
        let nearest_player = turns
            .single_player
            .unwrap_or_else(|| turns.nearest.at(self.at(i), position));
        // ServerLevel.tick: every loaded entity checks despawning, but
        // only those in entity-ticking chunks (within the simulation
        // distance) tick.
        if !(turns.ticking)(bx, bz) {
            turns.despawn.nearest_player = nearest_player;
            self.rest(i, &turns.despawn);
            return;
        }
        let controlling = self.controlling(i);
        // A mod canceled its tick: it stays as it is.
        if (turns.canceled)(self.mobs[i].mob().id) {
            self.keep.push(true);
            return;
        }
        // getCurrentDifficultyAt(blockPosition()): the mob's own chunk,
        // not the local player's (a server thread has no player).
        let chunk_key = (bx >> 4, bz >> 4);
        let difficulty = match turns.cached_difficulty {
            Some((key, difficulty)) if key == chunk_key => difficulty,
            _ => {
                let difficulty = (turns.difficulty)([position.x, position.y, position.z]);
                turns.cached_difficulty = Some((chunk_key, difficulty));
                difficulty
            }
        };
        let player = (seen_guest, nearest_player, controlling);
        self.tick(i, player, difficulty, turns.base, shared);
    }

    /// The guest mob `i` sees now, kept in the bindings: none while no guest is on.
    pub(super) fn bind_guest(&mut self, i: usize) -> Option<u32> {
        if self.guests.is_empty() {
            return None;
        }
        let m = self.mobs[i].mob();
        let bound = self.bindings.get(&m.id).copied();
        let conn = super::super::lan_mobs::player_for(m, self.player.as_ref(), &self.guests, bound);
        match conn {
            Some(conn) => self.bindings.insert(m.id, conn),
            None => self.bindings.remove(&m.id),
        };
        conn
    }

    /// Mob `i`'s turn when its chunk isn't loaded: it is kept as it is, unless it would despawn.
    fn unloaded(&mut self, i: usize) {
        let m = self.mobs[i].mob();
        self.keep.push(!m.hostile() || m.persistent);
    }

    /// Mob `i`'s turn outside the entity-ticking area: only its despawn check, which reads the
    /// nearest player and the difficulty, not the player the mob sees.
    fn rest(&mut self, i: usize, despawn_w: &MobWorld) {
        let m = self.mobs[i].mob_mut();
        let kept = horizon_client::mobs::ai::despawn_check(m, despawn_w);
        if !kept {
            self.died.remove(&m.id);
            self.snapshot[i].alive = false;
        }
        self.keep.push(kept);
    }

    /// Who controls mob `i`'s moves this tick: the velocity of the player riding it, when it is
    /// a controlled mount.
    fn controlling(&self, i: usize) -> Option<Vec3> {
        let m = self.mobs[i].mob();
        horizon_client::mobs::equine::controlled(m)
            .then(|| {
                self.player
                    .as_ref()
                    .filter(|p| p.riding_vehicle == Some(m.id))
                    .map(|p| p.velocity)
                    .or_else(|| {
                        self.guests
                            .iter()
                            .find(|(_, p)| p.riding_vehicle == Some(m.id))
                            .map(|(_, p)| p.velocity)
                    })
            })
            .flatten()
    }

    /// Mob `i`'s own tick, then what the loop does with what it did. `players`: the guest it
    /// sees (none: the host's player), its nearest player, and the velocity of the player
    /// controlling it as a mount ([`Self::controlling`]).
    fn tick(
        &mut self,
        i: usize,
        (seen_guest, nearest_player, controlling_player_movement): (
            Option<u32>,
            Option<Vec3>,
            Option<Vec3>,
        ),
        difficulty: LocalDifficulty,
        base: &MobWorld,
        shared: &Shared,
    ) {
        let m = self.mobs[i].mob_mut();
        horizon_client::mobs::apply_mob_pushes(m, &mut self.pushes);
        horizon_client::mobs::blocks_changed(m, shared.path_changes);
        let from = self.events.len();
        // The guest's view is lent to the mob for its tick, not copied:
        // the tick only reads it (`&MobWorld`), and it goes back to the
        // guest's slot straight after.
        let lent = seen_guest.and_then(|conn| self.guests.iter().position(|g| g.0 == conn));
        let active_player = match (seen_guest, lent) {
            (None, _) => self.player.take(),
            (Some(_), Some(k)) => self
                .spare
                .take()
                .map(|stand_in| std::mem::replace(&mut self.guests[k].1, stand_in)),
            (Some(_), None) => None,
        };
        let mut w = MobWorld {
            others: &self.snapshot,
            others_grid: Some(&self.grid),
            others_index: Some(&self.index),
            others_villagers: Some(&self.villager_slots),
            others_solid: Some(&self.solid_slots),
            difficulty,
            nearest_player,
            player: active_player,
            teams: std::sync::Arc::clone(&base.teams),
            ..*base
        };
        w.controlling_player_movement = controlling_player_movement;
        self.ticked.insert(self.mobs[i].mob().id);
        // `horizon-report`'s cost by mob type (tick_profile.rs).
        let sampled =
            super::super::tick_profile::start().map(|t| (t, self.mobs[i].mob().kind.clone()));
        let kept = horizon_client::mobs::riders::tick(
            &mut self.mobs,
            i,
            &w,
            &mut self.random,
            &mut self.events,
        );
        if let Some((started, kind)) = sampled {
            super::super::tick_profile::add(shared.level_id, &kind, started);
        }
        let m = self.mobs[i].mob_mut();
        if seen_guest.is_none() {
            self.player = w.player;
        } else if let Some(k) = lent {
            self.spare = w
                .player
                .map(|v| std::mem::replace(&mut self.guests[k].1, v));
        }
        // Its tick changed its target: the mods may keep the one it had
        // (`native_living_change_target`), and then what the tick did to
        // the new one is not dealt.
        if let Some(&before) = self.targets.get(i)
            && m.ai.target != before
        {
            let named = target_named(
                m.ai.target,
                seen_guest,
                &self.guests,
                shared.guest_ids,
                self.player.as_ref(),
            );
            if let Some((player, mob)) = named {
                let kind = super::super::native_hooks::mob_kind(m);
                let mut cancel = false;
                Game::native_living_change_target(
                    shared.level_id,
                    m.id,
                    &kind,
                    player,
                    mob,
                    &mut cancel,
                );
                if cancel {
                    if let Some(new) = m.ai.target {
                        let mut k = from;
                        while k < self.events.len() {
                            if hits_target(&self.events[k], new) {
                                self.events.remove(k);
                            } else {
                                k += 1;
                            }
                        }
                    }
                    m.ai.target = before;
                }
            }
            self.targets[i] = m.ai.target;
        }
        // A killed mob's body is removed: its effects that act on
        // death run (`LivingEntity.remove` with KILLED).
        if !kept && !m.alive() && !m.removed && !m.extra.effects.is_empty() {
            self.corpses.push(m.clone());
        }
        horizon_client::mobs::take_mob_pushes(&mut self.events, from, &mut self.pushes);
        // LivingEntity.tickDeath: after twenty ticks the body is removed.
        if !kept && !m.removed && !m.alive() && m.death_time >= 20 {
            let (width, height) = (f64::from(m.width()), f64::from(m.height()));
            // `AbstractCubeMob.remove` splits a grown cube into its smaller halves when it is
            // removed, twenty ticks after the death. The parent leaves the list before the split
            // runs, so its size, position, kind and persistence are captured here, as they are
            // at the removal.
            let split = if matches!(
                m.species(),
                Species::Slime | Species::MagmaCube | Species::SulfurCube
            ) && m.extra.size > 1
            {
                Some(m.clone())
            } else {
                None
            };
            self.gone.push((m.id, m.position, width, height, split));
        }
        if let Some(conn) = seen_guest {
            // ShulkerBullet.finalTarget is the guest's entity: the
            // bullet homes on it while the projectiles tick, where
            // `Who::Player` is the host's own player.
            if let Some(&(_, id)) = shared.guest_ids.iter().find(|g| g.0 == conn) {
                for e in &mut self.events[from..] {
                    if let MobEvent::Projectile(p) = e
                        && let ProjectileKind::ShulkerBullet { target, .. } = &mut p.kind
                        && *target == Some(Who::Player)
                    {
                        *target = Some(Who::Mob(id));
                    }
                }
            }
            let mut k = from;
            while k < self.events.len() {
                if super::super::lan_mobs::acts_on_player(&self.events[k]) {
                    self.guest_events.push((conn, m.id, self.events.remove(k)));
                } else {
                    k += 1;
                }
            }
        }
        // Killed during its own tick (drowning, burning, falling...).
        if !m.alive() && !m.removed && self.died.insert(m.id) {
            self.dead.push(m.clone());
        }
        if !kept {
            self.died.remove(&m.id);
        }
        self.keep.push(kept);
        // ServerLevel ticks entities one after another: the ones
        // after this mob see it as it is now.
        self.snapshot[i] = seen(m, shared.uuid);
        self.snapshot[i].alive &= kept;
        self.grid.update(i, self.snapshot[i].bounds);
        horizon_client::mobs::apply_brain_events(&mut self.mobs, &mut self.events, from);
    }
}
