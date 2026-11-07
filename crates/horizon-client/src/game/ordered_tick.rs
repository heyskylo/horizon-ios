//! PURPOSE: ticks experience orbs, primed TNT and falling blocks in the order
//! they joined the level, between the other kinds' batches.
//! WHY: `ServerLevel.tick` ticks every entity in join order, one kind among
//! the rest; these three used to tick as three fixed batches (orbs after the
//! mobs, falling blocks and TNT after the gateways), so a TNT that joined
//! before a boat blew up after the boat moved (#6509, tick order slice 2).
//! REFS: vanilla `ServerLevel.tick` (`entityTickList.forEach`),
//! `EntityTickList`; `join_seq.rs`; `tools/test_tick_order_parity.py`.
//! LOGS: none. READS-BY: `Game::tick_world`, `tick_mobs`.
//! ENDS-WHEN: every kind ticks from the join-order walk.

use super::{Game, Result};
use horizon_client::join_seq::{JoinSeq, Joined};
use horizon_core::geometry::Vec3;
use horizon_core::random::RandomSource as _;

/// What this tick has ticked of the three kinds.
#[derive(Default, Clone)]
pub(super) struct OrderedTicks {
    /// Joins at or above this started after the tick began; they tick next tick.
    born: JoinSeq,
    /// The last join number ticked, if any: later ones are still to come.
    pub(super) done: Option<JoinSeq>,
    /// Joins below this have had their spawn packet; the ones above joined
    /// since (a summoned falling block, a lit TNT) and tick after it goes out.
    pub(super) announced: JoinSeq,
    /// For each batch of another kind, in tick order (see [`Batch`]): the
    /// lowest join number among it and every batch after it.
    suffix_min: [JoinSeq; Batch::COUNT],
    /// The orbs' lava random, made on first use.
    lava_random: Option<horizon_core::random::LegacyRandom>,
}

/// The batches of the other kinds, in the order they tick.
#[derive(Clone, Copy)]
pub(super) enum Batch {
    ArmorStands,
    Boats,
    Carts,
    Tridents,
    Rockets,
    Items,
    Mobs,
    Projectiles,
}

impl Batch {
    const COUNT: usize = 8;
}

#[derive(Clone, Copy)]
enum Pending {
    Orb,
    Tnt,
    Falling,
}

/// The lowest join number in a batch, or none when it is empty.
pub(super) fn first_join<T: Joined>(list: &[T]) -> Option<JoinSeq> {
    list.iter().map(Joined::join_seq).min()
}

/// Where the entity that joined as `join` is in `list`, if still there. A list
/// is usually in join order (spawns append), so a binary search finds it; a
/// list put back out of order falls back to the walk.
fn find_join<T: Joined>(list: &[T], join: JoinSeq) -> Option<usize> {
    match list.binary_search_by_key(&join, Joined::join_seq) {
        // Join numbers are unique, so a hit is the entity even in a list out of order.
        Ok(i) => Some(i),
        Err(_) => list.iter().position(|e| e.join_seq() == join),
    }
}

impl Game<'_> {
    /// A level tick starts: what has joined so far ticks this tick.
    pub(super) fn begin_ordered_ticks(&mut self) {
        let firsts = [
            first_join(&self.armor_stands),
            first_join(&self.boats),
            first_join(&self.carts),
            first_join(&self.tridents),
            first_join(&self.rockets),
            first_join(&self.dropped),
            first_join(&self.mobs),
            first_join(&self.creatures.projectiles),
        ];
        // A batch ticks whole, so an entity of the three kinds goes after the
        // last batch that holds anything that joined before it.
        let mut suffix_min = [JoinSeq::MAX; Batch::COUNT];
        let mut lowest = JoinSeq::MAX;
        for (slot, first) in suffix_min.iter_mut().zip(firsts).rev() {
            lowest = lowest.min(first.unwrap_or(JoinSeq::MAX));
            *slot = lowest;
        }
        self.ordered = OrderedTicks {
            announced: self.ordered.announced,
            born: JoinSeq::upcoming(),
            done: None,
            suffix_min,
            lava_random: None,
        };
    }

    /// Ticks, one at a time in join order, every orb, primed TNT and falling
    /// block that joined before `cut` and has not ticked this tick.
    pub(super) fn run_ordered_ticks(&mut self, cut: JoinSeq, late: bool) -> Result<()> {
        let mut first_error = None;
        let mut followed = None;
        let mut events = false;
        let entity_ticking = self.entity_ticking_area();
        // Who ticks in this run, sorted once: entities that join meanwhile are
        // at or above `born`, so the set cannot grow, only lose the removed.
        let (done, born, announced) =
            (self.ordered.done, self.ordered.born, self.ordered.announced);
        let wanted = |j: JoinSeq| done.is_none_or(|d| j > d) && j < cut && j < born;
        let mut pending: Vec<(JoinSeq, Pending)> = Vec::new();
        let mut offer = |j: JoinSeq, kind: Pending| {
            // Orbs are announced after their tick, always; TNT and falling
            // blocks that joined since the last packets wait for them.
            let right_pass = match kind {
                Pending::Orb => !late,
                _ => late == (j >= announced),
            };
            if right_pass && wanted(j) {
                pending.push((j, kind));
            }
        };
        self.orbs.iter().for_each(|e| offer(e.join, Pending::Orb));
        self.tnts.iter().for_each(|e| offer(e.join, Pending::Tnt));
        self.falling
            .iter()
            .for_each(|e| offer(e.join, Pending::Falling));
        pending.sort_unstable_by_key(|&(j, _)| j);
        // EntityGetter.getEntityCollisions: the boxes that stop these moves (a
        // boat, a shulker, a happy ghast held still), gathered once for the
        // run and again after an explosion, which can move or remove them.
        let mut obstacles: Option<horizon_client::entities::ObstacleGrid> = None;
        let mut nearby = Vec::new();
        for (join, kind) in pending {
            self.ordered.done = Some(join);
            // ServerLevel.tick: only entities in entity-ticking chunks tick.
            match kind {
                Pending::Tnt => {
                    let Some(i) = find_join(&self.tnts, join) else {
                        continue;
                    };
                    let at = self.tnts[i].position;
                    if entity_ticking(at.x.floor() as i32, at.z.floor() as i32) {
                        let t = &self.tnts[i];
                        let area = t.bounds().expand_towards(t.velocity);
                        obstacles
                            .get_or_insert_with(|| {
                                horizon_client::entities::ObstacleGrid::new(
                                    self.item_entity_collisions(),
                                )
                            })
                            .near(area.inflate(Vec3::new(1.0, 1.0, 1.0)), &mut nearby);
                        let count = self.tnts.len();
                        if let Err(e) = self.tick_one_tnt(i, &nearby) {
                            first_error.get_or_insert(e);
                        }
                        if self.tnts.len() < count {
                            obstacles = None;
                        }
                    }
                }
                Pending::Falling => {
                    let Some(i) = find_join(&self.falling, join) else {
                        continue;
                    };
                    let at = self.falling[i].position;
                    if entity_ticking(at.x.floor() as i32, at.z.floor() as i32) {
                        events = true;
                        let f = &self.falling[i];
                        let area = horizon_client::falling_block::bounds(f.position)
                            .expand_towards(f.velocity);
                        obstacles
                            .get_or_insert_with(|| {
                                horizon_client::entities::ObstacleGrid::new(
                                    self.item_entity_collisions(),
                                )
                            })
                            .near(area.inflate(Vec3::new(1.0, 1.0, 1.0)), &mut nearby);
                        if let Err(e) = self.tick_one_falling(i, &nearby) {
                            first_error.get_or_insert(e);
                        }
                    }
                }
                Pending::Orb => {
                    let Some(i) = find_join(&self.orbs, join) else {
                        continue;
                    };
                    let followed = followed.get_or_insert_with(|| self.orb_followed());
                    // A block of slack around the move: gravity and the pull
                    // towards a player change it before it is made.
                    let o = &self.orbs[i];
                    let area = o.bounds().expand_towards(o.velocity);
                    obstacles
                        .get_or_insert_with(|| {
                            horizon_client::entities::ObstacleGrid::new(
                                self.item_entity_collisions(),
                            )
                        })
                        .near(area.inflate(Vec3::new(1.0, 1.0, 1.0)), &mut nearby);
                    let o = &mut self.orbs[i];
                    let distance = |feet: &[f64; 3]| -> f64 {
                        (feet[0] - o.position.x).powi(2)
                            + (feet[1] - o.position.y).powi(2)
                            + (feet[2] - o.position.z).powi(2)
                    };
                    let target = followed
                        .iter()
                        .filter(|(feet, _)| distance(feet) < 64.0)
                        .min_by(|a, b| distance(&a.0).total_cmp(&distance(&b.0)))
                        .map(|(_, target)| *target);
                    // An orb has no random of its own here: lava draws from one
                    // made on first use, apart from the level stream.
                    let random = &mut self.ordered.lava_random;
                    let mut next_float = || {
                        random
                            .get_or_insert_with(super::dispenser::entity_random)
                            .next_float()
                    };
                    let stays = {
                        let audio = &mut self.audio;
                        let mut on_sound = |pos: [f64; 3], pitch: f32| {
                            audio.play("minecraft:entity.generic.burn", Some(pos), 0.4, pitch);
                        };
                        o.tick_among(
                            &self.world,
                            self.res.facts(),
                            self.collision,
                            target,
                            self.remote.is_none(),
                            &mut next_float,
                            Some(self.border.geometry()),
                            &self.res.motion().fluids,
                            &mut on_sound,
                            &nearby,
                        )
                    };
                    // PowderSnowBlock.entityInside during the move, before an
                    // orb that is done is removed.
                    let snow = (o.previous, o.position, o.bounds(), o.velocity);
                    self.entity_powder_snow_particles(snow.0, snow.1, snow.2, snow.3, false);
                    if !stays {
                        self.orbs.remove(i);
                    }
                }
            }
        }
        if events && let Err(e) = self.process_level_events() {
            first_error.get_or_insert(e);
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Before a batch of another kind ticks: the three kinds' entities that
    /// joined before everything still to tick in the batches from this one on.
    pub(super) fn ordered_before(&mut self, batch: Batch) -> Result<()> {
        let cut = self.ordered.suffix_min[batch as usize];
        if cut == JoinSeq::MAX {
            return Ok(());
        }
        self.run_ordered_ticks(cut, false)
    }
}

#[cfg(test)]
mod tests {
    use super::find_join;
    use horizon_client::join_seq::{JoinList, JoinSeq, Joined as _};

    #[derive(Clone)]
    struct E {
        join: JoinSeq,
    }
    horizon_client::impl_joined!(E);

    /// The run looks each entity up by join number: in a list in join order,
    /// one put back out of order, and after the entity left.
    #[test]
    fn find_join_finds_by_number_in_or_out_of_order() {
        let mut list = JoinList::default();
        for _ in 0..5 {
            list.push(E {
                join: JoinSeq::UNJOINED,
            });
        }
        let joins: Vec<JoinSeq> = list.iter().map(|e| e.join_seq()).collect();
        for (i, &j) in joins.iter().enumerate() {
            assert_eq!(find_join(&list, j), Some(i));
        }
        list.swap(0, 4);
        list.swap(1, 3);
        assert_eq!(find_join(&list, joins[0]), Some(4));
        assert_eq!(find_join(&list, joins[4]), Some(0));
        assert_eq!(find_join(&list, joins[1]), Some(3));
        let gone = list.remove(2).join_seq();
        assert_eq!(find_join(&list, gone), None);
    }
}
