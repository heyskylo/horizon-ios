//! The chunks a `/forceload` or a native mod's ticket holds, as `Ranges` reads them, and their
//! distance map.
use super::{FORCED_FEATURES_RING, FORCED_FULL_RING, FORCED_TERRAIN_RING, Ranges};
use crate::fast_hash::FxHashMap;
use std::collections::HashSet;

/// `Streaming::forced` and `mod_forced` as the Chebyshev distance from each column within
/// `REACH` of one to the nearest: `Ranges::in_main_range` answers by one lookup instead of going
/// through every forced chunk (it is asked for every column, several times a tick).
pub(super) struct ForcedCover {
    forced: HashSet<(i32, i32)>,
    mod_forced: HashSet<(i32, i32)>,
    near: FxHashMap<(i32, i32), u8>,
}
impl ForcedCover {
    /// The widest ring `in_main_range` asks of a forced chunk (margins 1 to 4).
    const REACH: i32 = FORCED_TERRAIN_RING + 1;
    /// Past this many forced chunks the cover is not made: the scan answers.
    const MOST: usize = 4096;
    pub(super) fn refresh(
        cover: &mut Option<Self>,
        forced: &HashSet<(i32, i32)>,
        mod_forced: &HashSet<(i32, i32)>,
    ) {
        if cover
            .as_ref()
            .is_some_and(|c| c.forced == *forced && c.mod_forced == *mod_forced)
        {
            return;
        }
        if forced.len() + mod_forced.len() > Self::MOST {
            *cover = None;
            return;
        }
        let mut near = FxHashMap::default();
        for &(fx, fz) in forced.iter().chain(mod_forced) {
            for dx in -Self::REACH..=Self::REACH {
                for dz in -Self::REACH..=Self::REACH {
                    let d = dx.abs().max(dz.abs()) as u8;
                    near.entry((fx + dx, fz + dz))
                        .and_modify(|e: &mut u8| *e = (*e).min(d))
                        .or_insert(d);
                }
            }
        }
        *cover = Some(Self {
            forced: forced.clone(),
            mod_forced: mod_forced.clone(),
            near,
        });
    }
    /// Whether a forced chunk lies within `reach` of `(x, z)`, or `None` past `REACH`.
    pub(super) fn within(&self, x: i32, z: i32, reach: i32) -> Option<bool> {
        (reach <= Self::REACH).then(|| {
            self.near
                .get(&(x, z))
                .is_some_and(|&d| i32::from(d) <= reach)
        })
    }
}

impl Ranges<'_> {
    /// Whether a `/forceload` or a native mod's ticket holds the chunk.
    pub(super) fn is_forced(&self, x: i32, z: i32) -> bool {
        self.forced.contains(&(x, z)) || self.mod_forced.contains(&(x, z))
    }
    /// Every chunk a `/forceload` or a native mod's ticket holds, once each.
    pub(super) fn forced_chunks(&self) -> impl Iterator<Item = &(i32, i32)> + '_ {
        self.forced
            .iter()
            .chain(self.mod_forced.iter().filter(|c| !self.forced.contains(c)))
    }
    /// `in_main_range` past the player's circle: whether a forced chunk lies within the ring
    /// `margin` asks for.
    pub(super) fn near_forced(&self, x: i32, z: i32, margin: i32) -> bool {
        // A forced chunk holds a FORCED ticket at level 31
        // (`ChunkMap.FORCED_TICKET_LEVEL`), and the level rises one per ring
        // (`ChunkTracker.computeLevelFromNeighbor`): the margin is the
        // vanilla status ring the caller asks for.
        let reach = match margin {
            1 => FORCED_FULL_RING,
            2 => FORCED_FEATURES_RING,
            3 => FORCED_TERRAIN_RING,
            _ => margin + 1,
        };
        if let Some(hit) = self.forced_cover.and_then(|c| c.within(x, z, reach)) {
            return hit;
        }
        self.forced_chunks()
            .any(|f| (x - f.0).abs() <= reach && (z - f.1).abs() <= reach)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(seed: &mut u64, n: usize, spread: i32) -> HashSet<(i32, i32)> {
        let mut next = |m: i32| {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            (*seed % (2 * m as u64 + 1)) as i32 - m
        };
        (0..n).map(|_| (next(spread), next(spread))).collect()
    }

    /// With the forced chunks by distance, every column gets the answer the scan of every forced
    /// chunk gives, at every margin streaming asks (and those past the map's reach), after the
    /// forced chunks change too.
    #[test]
    fn the_distances_answer_as_the_scan() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut cover = None;
        for case in 0..24 {
            let spread = [3, 12, 40][case % 3];
            let forced = chunks(&mut seed, 1 + case % 9, spread);
            // Some mod tickets on forced chunks, some apart.
            let mut mod_forced = chunks(&mut seed, case % 4, spread);
            mod_forced.extend(forced.iter().take(case % 2));
            ForcedCover::refresh(&mut cover, &forced, &mod_forced);
            let made = cover.as_ref().expect("a few forced chunks get a map");
            let ranges = |forced_cover| Ranges {
                // The player's circle far away: only the forced chunks answer.
                center: (10_000, 10_000),
                radius: 2,
                extra: &[],
                forced: &forced,
                mod_forced: &mod_forced,
                cover: None,
                forced_cover,
                band: None,
            };
            let (scan, map) = (ranges(None), ranges(Some(made)));
            let span = spread + 10;
            for z in -span..=span {
                for x in -span..=span {
                    for margin in 0..=7 {
                        assert_eq!(
                            map.in_main_range(x, z, margin),
                            scan.in_main_range(x, z, margin),
                            "case {case} ({x}, {z}) margin {margin}"
                        );
                    }
                    for d in [false, true] {
                        assert_eq!(map.requested(x, z, d), scan.requested(x, z, d));
                        assert_eq!(map.chunk_kept(x, z, d), scan.chunk_kept(x, z, d));
                        assert_eq!(map.unloaded(x, z, d), scan.unloaded(x, z, d));
                    }
                }
            }
        }
    }

    /// Too many forced chunks for a map: none is made, and the scan answers.
    #[test]
    fn past_the_most_no_map_is_made() {
        let forced: HashSet<(i32, i32)> = (0..=ForcedCover::MOST as i32).map(|x| (x, 0)).collect();
        let mut cover = None;
        ForcedCover::refresh(&mut cover, &HashSet::new(), &HashSet::from([(0, 0)]));
        assert!(cover.is_some());
        ForcedCover::refresh(&mut cover, &forced, &HashSet::new());
        assert!(cover.is_none());
    }
}
