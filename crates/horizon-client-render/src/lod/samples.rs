//! The samples mesh jobs read (`Snapshot::samples`): the active tiles' ready columns, kept
//! as the tiles change rather than gathered again for every batch of jobs. Kept in shards of
//! nearby tiles behind `Arc`s: a batch's copy (`clone`) copies the shards' handles, and a
//! change copies only its shard, and only while a job still reads it.
use super::{Column, TileKey};
use crate::fast_hash::FxHashMap;
use std::sync::Arc;

type Shard = FxHashMap<TileKey, Arc<Vec<Column>>>;

/// 8 by 8 tiles of one level a shard.
fn shard_of(key: TileKey) -> (i32, i32, u8) {
    (key.x >> 3, key.z >> 3, key.level)
}

#[derive(Clone, Default)]
pub(super) struct Samples {
    shards: FxHashMap<(i32, i32, u8), Arc<Shard>>,
}

impl Samples {
    pub fn get(&self, key: &TileKey) -> Option<&Arc<Vec<Column>>> {
        self.shards.get(&shard_of(*key))?.get(key)
    }
    /// Sets `key`'s samples (`None`: not active or not ready), copying its shard only when
    /// they change.
    pub fn set(&mut self, key: TileKey, samples: Option<&Arc<Vec<Column>>>) {
        let at = shard_of(key);
        match samples {
            Some(samples) => {
                let shard = self.shards.entry(at).or_default();
                if shard.get(&key).is_none_or(|s| !Arc::ptr_eq(s, samples)) {
                    Arc::make_mut(shard).insert(key, samples.clone());
                }
            }
            None => {
                let Some(shard) = self.shards.get_mut(&at) else {
                    return;
                };
                if shard.contains_key(&key) {
                    Arc::make_mut(shard).remove(&key);
                    if shard.is_empty() {
                        self.shards.remove(&at);
                    }
                }
            }
        }
    }
    /// Keeps the tiles `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(&TileKey) -> bool) {
        self.shards.retain(|_, shard| {
            if !shard.keys().all(&mut keep) {
                Arc::make_mut(shard).retain(|key, _| keep(key));
            }
            !shard.is_empty()
        });
    }
    #[cfg(any(test, debug_assertions))]
    pub fn iter(&self) -> impl Iterator<Item = (&TileKey, &Arc<Vec<Column>>)> {
        self.shards.values().flat_map(|shard| shard.iter())
    }
}

impl FromIterator<(TileKey, Arc<Vec<Column>>)> for Samples {
    fn from_iter<I: IntoIterator<Item = (TileKey, Arc<Vec<Column>>)>>(iter: I) -> Self {
        let mut samples = Self::default();
        for (key, columns) in iter {
            samples.set(key, Some(&columns));
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32, z: i32, level: u8) -> TileKey {
        TileKey { x, z, level }
    }

    #[test]
    fn a_change_leaves_a_batchs_copy_as_it_was() {
        let a = Arc::new(Vec::new());
        let b = Arc::new(Vec::new());
        let mut live: Samples = [(key(0, 0, 0), a.clone()), (key(40, -3, 1), a.clone())]
            .into_iter()
            .collect();
        let batch = live.clone();
        live.set(key(0, 0, 0), Some(&b));
        live.set(key(40, -3, 1), None);
        live.set(key(-9, 2, 0), Some(&b));
        assert!(Arc::ptr_eq(batch.get(&key(0, 0, 0)).unwrap(), &a));
        assert!(batch.get(&key(40, -3, 1)).is_some());
        assert!(batch.get(&key(-9, 2, 0)).is_none());
        assert!(Arc::ptr_eq(live.get(&key(0, 0, 0)).unwrap(), &b));
        assert!(live.get(&key(40, -3, 1)).is_none());
        live.retain(|k| k.x < 0);
        assert_eq!(
            live.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            [key(-9, 2, 0)]
        );
        assert_eq!(batch.iter().count(), 2);
    }
}
