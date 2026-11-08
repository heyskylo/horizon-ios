//! Far tiles the plan let go, kept a while for a return: a teleport back, a
//! trip out and home again, the far distance lowered and raised. Taken back
//! (`Lod::replan`) instead of sampled again, which costs every far tile its
//! noise, biomes and surface rules a second time.
//!
//! Bounded in time (`KEPT_FOR`) and in memory (`KEPT_BYTES`); past the bytes
//! the finest go first, so the coarse shape of every place nearby stays
//! longest. Nothing is written to disk: the tiles leave with the `Lod`, that
//! is with the world.
use super::{Column, Span, TileKey};
use crate::fast_hash::FxHashMap;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a tile is kept after the plan let it go.
pub(super) const KEPT_FOR: Duration = Duration::from_secs(600);
/// About as many bytes as a 512-chunk view's tiles hold, so one whole place
/// is kept; a far tile is some 20 to 60 KB.
pub(super) const KEPT_BYTES: usize = 512 << 20;

struct Kept {
    columns: Arc<Vec<Column>>,
    /// Its detail (`Lod::detail_masks`), so refined cells are not asked again.
    mask: Option<[u64; 4]>,
    at: Instant,
    bytes: usize,
}

pub(super) struct KeptTiles {
    tiles: FxHashMap<TileKey, Kept>,
    bytes: usize,
    checked: Instant,
    time: Duration,
    budget: usize,
}

impl KeptTiles {
    pub(super) fn new(time: Duration, budget: usize) -> Self {
        Self {
            tiles: FxHashMap::default(),
            bytes: 0,
            checked: Instant::now(),
            time,
            budget,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.tiles.len()
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn contains(&self, key: &TileKey) -> bool {
        self.tiles.contains_key(key)
    }

    pub(super) fn keep(&mut self, key: TileKey, columns: Arc<Vec<Column>>, mask: Option<[u64; 4]>) {
        // Spans past the inline four live on the heap.
        let bytes = std::mem::size_of_val(columns.as_slice())
            + columns
                .iter()
                .filter(|column| column.spans.len() > 4)
                .map(|column| column.spans.len() * std::mem::size_of::<Span>())
                .sum::<usize>();
        let kept = Kept {
            columns,
            mask,
            at: Instant::now(),
            bytes,
        };
        self.bytes += bytes;
        if let Some(old) = self.tiles.insert(key, kept) {
            self.bytes -= old.bytes;
        }
    }

    pub(super) fn take(&mut self, key: &TileKey) -> Option<(Arc<Vec<Column>>, Option<[u64; 4]>)> {
        let kept = self.tiles.remove(key)?;
        self.bytes -= kept.bytes;
        Some((kept.columns, kept.mask))
    }

    /// Forgets the tiles `stale` picks: their source has more than they hold.
    pub(super) fn forget(&mut self, mut stale: impl FnMut(&TileKey, &[Column]) -> bool) {
        let bytes = &mut self.bytes;
        self.tiles.retain(|key, kept| {
            let keep = !stale(key, &kept.columns);
            if !keep {
                *bytes -= kept.bytes;
            }
            keep
        });
    }

    /// Whether a minute went by since the last `trim`.
    pub(super) fn due(&self) -> bool {
        !self.tiles.is_empty() && self.checked.elapsed() >= Duration::from_secs(60)
    }

    /// Drops the tiles kept past their time, then, past the budget, the
    /// finest and the oldest first.
    pub(super) fn trim(&mut self) {
        self.checked = Instant::now();
        let (bytes, time) = (&mut self.bytes, self.time);
        self.tiles.retain(|_, kept| {
            let keep = kept.at.elapsed() < time;
            if !keep {
                *bytes -= kept.bytes;
            }
            keep
        });
        if self.bytes <= self.budget {
            return;
        }
        let mut order: Vec<_> = self
            .tiles
            .iter()
            .map(|(key, kept)| (key.level, kept.at, *key))
            .collect();
        order.sort_unstable();
        for (_, _, key) in order {
            if self.bytes <= self.budget {
                break;
            }
            self.take(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lod::{CELLS, Surface};
    use horizon_world::registry::BlockStateId;

    fn tile(height: i32) -> Arc<Vec<Column>> {
        Arc::new(vec![
            Column::from_surface(Some(Surface {
                height,
                state: BlockStateId(1),
                water: None,
                biome: 0,
            }));
            CELLS * CELLS
        ])
    }

    fn key(level: u8) -> TileKey {
        TileKey { x: 3, z: -2, level }
    }

    #[test]
    fn a_kept_tile_comes_back_with_its_detail_once() {
        let mut kept = KeptTiles::new(KEPT_FOR, usize::MAX);
        kept.keep(key(0), tile(64), Some([u64::MAX; 4]));
        let (columns, mask) = kept.take(&key(0)).unwrap();
        assert_eq!(columns[0].surface.unwrap().height, 64);
        assert_eq!(mask, Some([u64::MAX; 4]));
        assert!(kept.take(&key(0)).is_none());
        assert_eq!(kept.bytes(), 0);
    }

    #[test]
    fn past_the_budget_the_finest_go_first_and_past_the_time_all() {
        let mut kept = KeptTiles::new(KEPT_FOR, usize::MAX);
        kept.keep(key(4), tile(80), None);
        let one = kept.bytes();
        kept.keep(key(0), tile(64), None);
        kept.keep(key(1), tile(70), None);
        kept.budget = one * 2;
        kept.trim();
        assert!(kept.contains(&key(4)) && kept.contains(&key(1)) && !kept.contains(&key(0)));
        assert_eq!(kept.bytes(), one * 2);
        kept.time = Duration::ZERO;
        kept.trim();
        assert_eq!((kept.len(), kept.bytes()), (0, 0));
    }

    #[test]
    fn forgotten_tiles_are_not_taken_back() {
        let mut kept = KeptTiles::new(KEPT_FOR, usize::MAX);
        kept.keep(key(2), tile(64), None);
        kept.keep(key(3), tile(64), None);
        kept.forget(|key, _| key.level == 2);
        assert!(kept.take(&key(2)).is_none());
        assert!(kept.take(&key(3)).is_some());
        assert_eq!(kept.bytes(), 0);
    }
}
