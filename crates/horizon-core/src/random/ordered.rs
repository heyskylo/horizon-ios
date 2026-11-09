//! One [`LegacyRandom`] stream shared by several threads that each run their part of a list in list
//! order (the server's parallel mob tick, `game/mob_islands.rs`): every draw comes out exactly as if
//! the whole list had run on one thread.
//!
//! Each thread (a lane) runs a subset of the list's items, in ascending item order, and says which
//! item it is on ([`LegacyRandom::begin_item`]). The first draw an item makes waits until every other
//! lane has passed that item, then takes the stream's state; the item's end gives it back
//! ([`LegacyRandom::end_item`]). So the items before it have made all their draws and the items after
//! it none. The lane on the smallest item never waits, so the lanes can't deadlock, as long as every
//! lane has its own thread. An item that never draws never waits.
use super::LegacyRandom;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicUsize, Ordering},
};

/// The shared stream and where each lane is.
#[derive(Debug)]
pub struct OrderedDraws {
    /// The stream's state between holders: the generator state and the gaussian cache.
    state: Mutex<(u64, Option<f64>)>,
    /// The item each lane is on; `usize::MAX` once it has finished.
    lanes: Box<[AtomicUsize]>,
}

/// A lane's hold on the shared stream, inside its [`LegacyRandom`].
#[derive(Debug)]
pub(super) struct Gate {
    draws: Arc<OrderedDraws>,
    lane: usize,
    item: usize,
    held: bool,
}

impl OrderedDraws {
    /// `random`'s stream shared by `lanes` lanes, none of them started.
    pub fn new(random: &LegacyRandom, lanes: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((random.state, random.gaussian)),
            lanes: (0..lanes).map(|_| AtomicUsize::new(0)).collect(),
        })
    }
    /// The random lane `lane` draws from: its draws wait for their turn.
    pub fn lane(self: &Arc<Self>, lane: usize) -> LegacyRandom {
        let mut random = LegacyRandom::from_state(0);
        random.gate = Some(Box::new(Gate {
            draws: Arc::clone(self),
            lane,
            item: 0,
            held: false,
        }));
        random
    }
    /// The stream as the lanes left it, once they have all finished.
    pub fn finish(&self) -> LegacyRandom {
        let (state, gaussian) = *self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut random = LegacyRandom::from_state(state);
        random.gaussian = gaussian;
        random
    }
    /// Wait until lane `lane` has finished item `item` (it is on a later one, or done).
    pub fn wait_past(&self, lane: usize, item: usize) {
        let mut spins = 0u32;
        while self.lanes[lane].load(Ordering::Acquire) <= item {
            spins += 1;
            if spins < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }
    fn wait_for_turn(&self, lane: usize, item: usize) {
        let mut spins = 0u32;
        loop {
            let ready = self
                .lanes
                .iter()
                .enumerate()
                .all(|(k, at)| k == lane || at.load(Ordering::Acquire) > item);
            if ready {
                return;
            }
            spins += 1;
            if spins < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }
}

impl Gate {
    /// The shared state for this item's first draw, after the items before it.
    pub(super) fn take(&mut self) -> (u64, Option<f64>) {
        self.draws.wait_for_turn(self.lane, self.item);
        self.held = true;
        *self
            .draws
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    pub(super) fn held(&self) -> bool {
        self.held
    }
}

impl LegacyRandom {
    /// A gated random's lane starts item `item` (ascending within the lane). Nothing for an
    /// ungated one.
    pub fn begin_item(&mut self, item: usize) {
        if let Some(gate) = self.gate.as_deref_mut() {
            gate.item = item;
            gate.draws.lanes[gate.lane].store(item, Ordering::Release);
        }
    }
    /// The item's draws are done: the shared stream goes on from where it left it.
    pub fn end_item(&mut self) {
        if let Some(gate) = self.gate.as_deref_mut()
            && gate.held
        {
            *gate
                .draws
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = (self.state, self.gaussian);
            gate.held = false;
        }
    }
    /// The lane has run all its items: no other lane waits for it any more.
    pub fn finish_lane(&mut self) {
        self.end_item();
        if let Some(gate) = self.gate.as_deref_mut() {
            gate.draws.lanes[gate.lane].store(usize::MAX, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::RandomSource;

    /// Items 0..40 over three lanes, each drawing on some items: the same values as one thread.
    #[test]
    fn lanes_draw_in_list_order() {
        let draws_on = |item: usize| match item % 5 {
            0 => 0,
            1 => 1,
            2 => 3,
            _ => (item % 3) as i32,
        };
        let mut serial = LegacyRandom::new(42);
        let expected: Vec<Vec<i32>> = (0..40)
            .map(|i| {
                (0..draws_on(i))
                    .map(|_| serial.next_int_bound(1000).unwrap())
                    .collect()
            })
            .collect();
        let shared = OrderedDraws::new(&LegacyRandom::new(42), 3);
        let got: Vec<(usize, Vec<i32>)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..3)
                .map(|lane| {
                    let shared = Arc::clone(&shared);
                    s.spawn(move || {
                        let mut random = shared.lane(lane);
                        let mut out = Vec::new();
                        for item in (0..40).filter(|i| (i * 7 + 3) % 3 == lane) {
                            random.begin_item(item);
                            let values = (0..draws_on(item))
                                .map(|_| random.next_int_bound(1000).unwrap())
                                .collect();
                            random.end_item();
                            out.push((item, values));
                        }
                        random.finish_lane();
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });
        let mut got = got;
        got.sort_by_key(|(i, _)| *i);
        assert_eq!(
            got.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(shared.finish().state(), serial.state());
    }
}
