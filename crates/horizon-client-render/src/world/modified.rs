//! `LevelChunk.markUnsaved` and `ChunkMap`'s insertion-ordered eager queue.
use crate::fast_hash::{FxHashMap, FxHashSet};
use std::{
    collections::BTreeMap,
    ops::Deref,
    time::{Duration, Instant},
};

type Key = (i32, i32);
const COOLDOWN: Duration = Duration::from_millis(10_000);

#[derive(Default)]
pub struct ModifiedChunks {
    dirty: FxHashSet<Key>,
    // Ordered independently of the dirty set: an autosave leaves queue entries
    // in place until the eager loop visits them, including cooled-down ones.
    order: BTreeMap<u64, Key>,
    queued: FxHashMap<Key, u64>,
    sequence: u64,
    next_save: FxHashMap<Key, Instant>,
}
impl Deref for ModifiedChunks {
    type Target = FxHashSet<Key>;
    fn deref(&self) -> &Self::Target {
        &self.dirty
    }
}
impl IntoIterator for ModifiedChunks {
    type Item = Key;
    type IntoIter = std::collections::hash_set::IntoIter<Key>;
    fn into_iter(self) -> Self::IntoIter {
        self.dirty.into_iter()
    }
}
impl ModifiedChunks {
    pub fn insert(&mut self, key: Key) -> bool {
        let changed = self.dirty.insert(key);
        if changed && !self.queued.contains_key(&key) {
            self.sequence += 1;
            self.order.insert(self.sequence, key);
            self.queued.insert(key, self.sequence);
        }
        changed
    }
    pub fn extend(&mut self, keys: impl IntoIterator<Item = Key>) {
        for key in keys {
            self.insert(key);
        }
    }
    pub fn remove(&mut self, key: &Key) -> bool {
        self.dirty.remove(key)
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Key> + '_ {
        self.dirty.drain()
    }
    /// Like `drain`: queue entries of chunks no longer dirty go at the next eager pass.
    pub fn clear(&mut self) {
        self.dirty.clear();
    }
    /// `ChunkMap.scheduleUnload`: forget the cooldown, including clean chunks.
    pub fn unloaded(&mut self, key: &Key) {
        self.next_save.remove(key);
    }
    /// `ChunkMap.saveAllChunks(false)`: clear cooldowns, then save all dirty
    /// full chunks without a limit, starting a fresh cooldown for each write.
    pub fn autosaved(&mut self, keys: &[Key], now: Instant) {
        self.next_save.clear();
        self.next_save
            .extend(keys.iter().map(|&key| (key, now + COOLDOWN)));
    }
    /// `ChunkMap.saveChunksEagerly` / `saveChunkIfNeeded`. `available` checks
    /// both haveTime and activeChunkWrites before EACH candidate; `save` takes
    /// an immutable snapshot and queues it. Failed submissions stay dirty.
    pub fn save_eagerly(
        &mut self,
        now: Instant,
        mut available: impl FnMut() -> bool,
        mut loaded: impl FnMut(Key) -> bool,
        mut save: impl FnMut(Key) -> bool,
    ) {
        let mut saved = 0;
        let mut remove = Vec::new();
        for (&sequence, &key) in &self.order {
            if saved == 20 || !available() {
                break;
            }
            if !loaded(key) || !self.dirty.contains(&key) {
                remove.push((sequence, key));
            } else if self.next_save.get(&key).is_none_or(|&at| now >= at) && save(key) {
                self.dirty.remove(&key);
                self.next_save.insert(key, now + COOLDOWN);
                remove.push((sequence, key));
                saved += 1;
            }
        }
        for (sequence, key) in remove {
            self.order.remove(&sequence);
            self.queued.remove(&key);
        }
    }
}
