//! How a forced chunk's ring is ranked (#13175).
use super::{Band, FORCED_TERRAIN_RING, Ranges, Streaming};
use std::collections::HashSet;

type Chunks = HashSet<(i32, i32)>;

/// What queued jobs were ranked from (`Streaming::requeue`): the centre, the
/// band, the extra areas, and the forced chunks.
#[derive(Default)]
pub(super) struct Basis {
    player: ((i32, i32), Option<Band>),
    extra: Vec<((i32, i32), i32)>,
    forced: (Chunks, Chunks),
}

/// Whether the queued jobs' ranking is out of date; it is brought up to date.
pub(super) fn basis_changed(s: &mut Streaming) -> bool {
    let b = &mut s.queue_basis;
    let same = b.player == (s.center, s.band)
        && b.extra == s.extra
        && b.forced.0 == s.forced
        && b.forced.1 == s.mod_forced;
    if !same {
        b.player = (s.center, s.band);
        b.extra.clone_from(&s.extra);
        b.forced.0.clone_from(&s.forced);
        b.forced.1.clone_from(&s.mod_forced);
    }
    !same
}

impl Ranges<'_> {
    /// `player_distance`, or the nearer one to a forced chunk whose ring reaches
    /// (x, z): see `forced_distance`.
    pub(super) fn distance(&self, x: i32, z: i32) -> i64 {
        self.player_distance(x, z).min(self.forced_distance(x, z))
    }

    /// Distance (squared) to the nearest forced chunk whose ring reaches
    /// (x, z), or `i64::MAX`. A forced chunk's ring is loaded with its ticket
    /// (levels 32 to 35 around the FORCED ticket's 31), so the 3x3 it needs to
    /// tick is ranked around it, not by how far it lies from the player: vanilla
    /// ticks the chunk on the first tick (`ChunkMap.prepareTickingChunk`).
    pub(super) fn forced_distance(&self, x: i32, z: i32) -> i64 {
        const SIDE: usize = (2 * FORCED_TERRAIN_RING + 1) as usize;
        let held = self.forced.len() + self.mod_forced.len();
        if held == 0 {
            return i64::MAX;
        }
        let d = |f: (i32, i32)| {
            let (dx, dz) = (i64::from(x - f.0), i64::from(z - f.1));
            dx * dx + dz * dz
        };
        if held <= SIDE * SIDE {
            return self
                .forced_chunks()
                .filter(|f| (x - f.0).abs().max((z - f.1).abs()) <= FORCED_TERRAIN_RING)
                .map(|&f| d(f))
                .min()
                .unwrap_or(i64::MAX);
        }
        // Many forced chunks: look at the columns around (x, z) instead.
        (-FORCED_TERRAIN_RING..=FORCED_TERRAIN_RING)
            .flat_map(|dz| (-FORCED_TERRAIN_RING..=FORCED_TERRAIN_RING).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| (x + dx, z + dz))
            .filter(|f| self.is_forced(f.0, f.1))
            .map(d)
            .min()
            .unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{GenerationInputs, GenerationScan, NO_CHUNKS};
    use super::*;
    use crate::fast_hash::{FxHashMap, FxHashSet};
    use crate::world::World;

    /// A forced chunk's ring is loaded with its ticket, so it ranks around
    /// the forced chunk and not by how far it lies from the player. Vanilla
    /// then ticks the chunk on the first tick after a reload
    /// (`ChunkMap.prepareTickingChunk`: its 3x3 is full at once); ranked from
    /// the player at the origin, a ring 4096 blocks out arrived west/north
    /// first, over tens of ticks.
    #[test]
    fn a_forced_chunks_ring_is_ranked_around_it() {
        let ranges = |forced| Ranges {
            center: (0, 0),
            radius: 2,
            extra: &[],
            forced,
            mod_forced: &NO_CHUNKS,
            cover: None,
            forced_cover: None,
            band: None,
        };
        let forced: HashSet<(i32, i32)> = [(256, 256)].into();
        let r = ranges(&forced);
        // Squared distance to the forced chunk, by hand: (255, 254) is
        // one west and two north of it.
        assert_eq!(r.priority(256, 256), 0);
        assert_eq!(r.priority(255, 254), 1 + 4);
        assert_eq!(r.priority(260, 252), 16 + 16);
        // Past the fourth ring (level 36 and up) the player's distance rules,
        // as does the whole map without a forced chunk.
        assert_eq!(r.priority(261, 256), 261 * 261 + 256 * 256);
        let none = HashSet::new();
        assert_eq!(ranges(&none).priority(255, 254), 255 * 255 + 254 * 254);
        // Many forced chunks (the neighbour lookups) agree with few.
        let many: HashSet<(i32, i32)> = (0..100).map(|i| (256 + 20 * i, 256)).collect();
        assert_eq!(ranges(&many).priority(255, 254), 5);
        assert_eq!(ranges(&many).priority(276, 258), 4);
        // The generation plan fetches the 3x3 the chunk ticks with first.
        let world = World::new(horizon_world::registry::BlockStateId(0), 0);
        let (generating, finishing) = (FxHashSet::default(), FxHashSet::default());
        let protos = FxHashMap::default();
        let input = GenerationInputs {
            ranges: ranges(&forced),
            world: &world,
            generating: &generating,
            protos: &protos,
            finishing: &finishing,
            decorates: true,
            remote: false,
        };
        let candidates = GenerationScan::default()
            .candidates(input, usize::MAX)
            .expect("a scan");
        let ring: Vec<(i32, i32)> = candidates
            .iter()
            .map(|&(_, x, z)| (x, z))
            .filter(|&(x, z)| (x - 256).abs().max((z - 256).abs()) <= FORCED_TERRAIN_RING)
            .collect();
        assert_eq!(ring.len(), 81);
        assert_eq!(ring[0], (256, 256));
        let d = |(x, z): (i32, i32)| (x - 256).pow(2) + (z - 256).pow(2);
        for pair in ring.windows(2) {
            assert!(
                d(pair[0]) <= d(pair[1]),
                "{:?} before {:?}",
                pair[0],
                pair[1]
            );
        }
        let first_nine: HashSet<(i32, i32)> = ring[..9].iter().copied().collect();
        let expected: HashSet<(i32, i32)> = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| (256 + dx, 256 + dz)))
            .collect();
        assert_eq!(first_nine, expected);
    }

    /// Jobs queued before a chunk was forced follow it once it is
    /// (`Streaming::requeue` ranks them again).
    #[test]
    fn queued_jobs_follow_a_forced_chunk() {
        use super::super::{Job, Queued, rank_again};
        use std::collections::BinaryHeap;
        let job = |x, z, order| Queued {
            priority: 0,
            at: Some((x, z)),
            bias: 0,
            order,
            job: Job::Generate(x, z),
        };
        let first_after = |forced: &HashSet<(i32, i32)>| {
            let ranges = Ranges {
                center: (0, 0),
                radius: 2,
                extra: &[],
                forced,
                mod_forced: &NO_CHUNKS,
                cover: None,
                forced_cover: None,
                band: None,
            };
            // (255, 254) lies nearer the origin than (258, 256), the
            // forced chunk's neighbour two chunks east.
            let mut queue = BinaryHeap::from(vec![job(255, 254, 1), job(258, 256, 2)]);
            rank_again(&mut queue, |x, z| ranges.priority(x, z));
            queue.pop().and_then(|q| q.at)
        };
        assert_eq!(first_after(&HashSet::new()), Some((255, 254)));
        assert_eq!(first_after(&[(256, 256)].into()), Some((258, 256)));
    }
}
