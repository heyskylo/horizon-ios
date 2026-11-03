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
        loop {
            let (done, born, announced) =
                (self.ordered.done, self.ordered.born, self.ordered.announced);
            let wanted = |j: JoinSeq| done.is_none_or(|d| j > d) && j < cut && j < born;
            let mut next: Option<(JoinSeq, Pending)> = None;
            let mut offer = |j: JoinSeq, kind: Pending| {
                // Orbs are announced after their tick, always; TNT and falling
                // blocks that joined since the last packets wait for them.
                let right_pass = match kind {
                    Pending::Orb => !late,
                    _ => late == (j >= announced),
                };
                if right_pass && wanted(j) && next.is_none_or(|(n, _)| j < n) {
                    next = Some((j, kind));
                }
            };
            self.orbs.iter().for_each(|e| offer(e.join, Pending::Orb));
            self.tnts.iter().for_each(|e| offer(e.join, Pending::Tnt));
            self.falling
                .iter()
                .for_each(|e| offer(e.join, Pending::Falling));
            let Some((join, kind)) = next else { break };
            self.ordered.done = Some(join);
            // ServerLevel.tick: only entities in entity-ticking chunks tick.
            match kind {
                Pending::Tnt => {
                    let Some(i) = self.tnts.iter().position(|e| e.join == join) else {
                        continue;
                    };
                    let at = self.tnts[i].position;
                    if entity_ticking(at.x.floor() as i32, at.z.floor() as i32)
                        && let Err(e) = self.tick_one_tnt(i)
                    {
                        first_error.get_or_insert(e);
                    }
                }
                Pending::Falling => {
                    let Some(i) = self.falling.iter().position(|e| e.join == join) else {
                        continue;
                    };
                    let at = self.falling[i].position;
                    if entity_ticking(at.x.floor() as i32, at.z.floor() as i32) {
                        events = true;
                        if let Err(e) = self.tick_one_falling(i) {
                            first_error.get_or_insert(e);
                        }
                    }
                }
                Pending::Orb => {
                    let Some(i) = self.orbs.iter().position(|e| e.join == join) else {
                        continue;
                    };
                    let followed = followed.get_or_insert_with(|| self.orb_followed());
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
                    let stays = o.tick(
                        &self.world,
                        self.res.facts(),
                        self.collision,
                        target,
                        self.remote.is_none(),
                        &mut next_float,
                        Some(self.border.geometry()),
                        &self.res.motion().fluids,
                    );
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
