//! Chunks a native mod keeps loaded (NeoForge's `TicketController`): a train
//! running where nobody is, a long contraption, a machine that must keep
//! working (docs/native-mods.md, "Chunk tickets"). A held chunk is as a
//! `/forceload`ed one, its level stays open, but `/forceload` neither lists
//! nor releases it and it isn't saved.
use super::{Dim, Game};

use crate::mod_api::NativeChunkLoad;

#[cfg(test)]
#[path = "native_chunk_load_tests.rs"]
mod native_chunk_load_tests;

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// Holds chunk `chunk` (`[x, z]`, block coordinates `>> 4`) of the level
    /// in place loaded and ticking for `owner`, or, with `hold` false, lets
    /// go of it. `owner` names what holds it, under the mod's namespace
    /// (`create:train/4f0c…`, `create:contraption/12,64,-3`): the chunk stays
    /// while any owner holds it, as NeoForge's `TicketController.forceChunk`
    /// with an owner. A held chunk is loaded with the ring around it, its
    /// blocks and entities tick as a `/forceload`ed one's do, and its level
    /// stays open with nobody there. Tickets are kept until the owner lets go
    /// or the server stops, not saved: keep what you hold (a level
    /// attachment) and hold it again in `native_level_load`. True when the
    /// set of owners changed; false for an owner without a namespace, a chunk
    /// past the world's edge (as `/forceload` refuses it) and off a server. API 412.
    pub(super) fn native_force_chunk(&mut self, owner: &str, chunk: [i32; 2], hold: bool) -> bool {
        self.native_chunk_ticket(self.dim, owner, chunk, hold)
    }

    /// `native_force_chunk` for the level `dimension` (`minecraft:the_nether`),
    /// whether it is in place, open with nobody in it, or not open yet: a
    /// level held this way opens at the next server tick and ticks from then
    /// on, so a mod can generate or run the Nether and the End from any
    /// server hook. False also for a dimension other than the Overworld, the
    /// Nether and the End. API 413.
    pub(super) fn native_force_chunk_in(
        &mut self,
        owner: &str,
        dimension: &str,
        chunk: [i32; 2],
        hold: bool,
    ) -> bool {
        let Some(dim) = native_dim(dimension) else {
            return false;
        };
        self.native_chunk_ticket(dim, owner, chunk, hold)
    }

    /// Asks for chunk `chunk` of the level `dimension` to be loaded (to
    /// `FULL`, with its ring) without waiting for it, as Paper's
    /// `getChunkAtAsync` with a plugin ticket: the server thread does not
    /// block, the chunk comes in over the next ticks. It stays loaded and
    /// ticking, as a `native_force_chunk_in` ticket's does, until
    /// `native_chunk_load_release` for the same `owner`, namespaced as
    /// there. Poll `native_chunk_load_state`. True when this owner's request
    /// is new; false when it already had one or the request is refused. API
    /// @API:chunk_load_async@.
    pub(super) fn native_chunk_load_request(
        &mut self,
        owner: &str,
        dimension: &str,
        chunk: [i32; 2],
    ) -> bool {
        self.native_force_chunk_in(owner, dimension, chunk, true)
    }

    /// Where `owner`'s request for `chunk` of `dimension` stands
    /// (`NativeChunkLoad`). Loads nothing. API @API:chunk_load_async@.
    pub(super) fn native_chunk_load_state(
        &self,
        owner: &str,
        dimension: &str,
        chunk: [i32; 2],
    ) -> NativeChunkLoad {
        let asked = native_dim(dimension).is_some_and(|dim| {
            self.mod_chunk_tickets
                .get(&dim)
                .and_then(|level| level.get(&(chunk[0], chunk[1])))
                .is_some_and(|owners| owners.contains(owner))
        });
        if !asked || !self.native_authoritative() {
            NativeChunkLoad::Failed
        } else if self.native_chunk_is_loaded(dimension, chunk) {
            NativeChunkLoad::Loaded
        } else {
            NativeChunkLoad::Pending
        }
    }

    /// Whether chunk `chunk` of the level `dimension` is loaded now, in
    /// place or in a level open with nobody in it. Loads and generates
    /// nothing, and a level not open stays closed. False off a server. API
    /// @API:chunk_load_async@.
    pub(super) fn native_chunk_is_loaded(&self, dimension: &str, chunk: [i32; 2]) -> bool {
        let Some(dim) = native_dim(dimension).filter(|_| self.native_authoritative()) else {
            return false;
        };
        let world = if dim == self.dim {
            &self.world
        } else if let Some(world) = self.levels.parked_world(dim) {
            world
        } else {
            return false;
        };
        world.chunks.contains_key(&(chunk[0], chunk[1]))
    }

    /// Lets go of `owner`'s request for `chunk` of `dimension`, so the chunk
    /// can unload again once nothing else (a player, `/forceload`, another
    /// owner) holds it. True when `owner` had one. API @API:chunk_load_async@.
    pub(super) fn native_chunk_load_release(
        &mut self,
        owner: &str,
        dimension: &str,
        chunk: [i32; 2],
    ) -> bool {
        self.native_force_chunk_in(owner, dimension, chunk, false)
    }

    /// Hold or release `owner`'s ticket on `chunk` of `dim`'s level, and
    /// hand the level's held chunks to its streaming if it is open.
    fn native_chunk_ticket(&mut self, dim: Dim, owner: &str, chunk: [i32; 2], hold: bool) -> bool {
        let namespaced = owner
            .split_once(':')
            .is_some_and(|(ns, path)| !ns.is_empty() && !path.is_empty());
        // `/forceload`'s world: blocks -30,000,000 to 29,999,999, so a mod's
        // corrupt saved list asks for nothing past the edge (where the ring's
        // arithmetic would overflow).
        let in_world = chunk.iter().all(|c| (-1_875_000..1_875_000).contains(c));
        if !namespaced || !self.native_authoritative() || (hold && !in_world) {
            return false;
        }
        let key = (chunk[0], chunk[1]);
        let level = self.mod_chunk_tickets.entry(dim).or_default();
        let changed = if hold {
            level.entry(key).or_default().insert(owner.to_owned())
        } else {
            let removed = level.get_mut(&key).is_some_and(|o| o.remove(owner));
            if level.get(&key).is_some_and(|o| o.is_empty()) {
                level.remove(&key);
            }
            removed
        };
        if !changed {
            return false;
        }
        let held = level.contains_key(&key);
        // A level not open yet gets its tickets when it opens
        // (`native_chunk_tickets_opened`).
        let streaming = if dim == self.dim {
            Some(&mut self.streaming)
        } else {
            self.levels.parked_streaming_mut(dim)
        };
        if let Some(streaming) = streaming {
            if !held {
                streaming.mod_forced.remove(&key);
            } else if streaming.mod_forced.insert(key) {
                streaming.rank_forced(key.0, key.1, key.0, key.1);
            }
        }
        true
    }

    /// The level just opened in place takes the tickets mods hold for it.
    pub(super) fn native_chunk_tickets_opened(&mut self) {
        let Some(level) = self.mod_chunk_tickets.get(&self.dim) else {
            return;
        };
        for &key in level.keys() {
            if self.streaming.mod_forced.insert(key) {
                self.streaming.rank_forced(key.0, key.1, key.0, key.1);
            }
        }
    }

    /// The chunks of the level in place `owner` holds (`native_force_chunk`),
    /// sorted. API 412.
    pub(super) fn native_forced_chunks(&self, owner: &str) -> Vec<[i32; 2]> {
        let mut held: Vec<[i32; 2]> = self
            .mod_chunk_tickets
            .get(&self.dim)
            .into_iter()
            .flatten()
            .filter(|(_, owners)| owners.contains(owner))
            .map(|(&(x, z), _)| [x, z])
            .collect();
        held.sort_unstable();
        held
    }
}

/// The level a dimension id names, for the three Horizon has.
fn native_dim(dimension: &str) -> Option<Dim> {
    match dimension {
        "minecraft:overworld" => Some(Dim::Overworld),
        "minecraft:the_nether" => Some(Dim::Nether),
        "minecraft:the_end" => Some(Dim::End),
        _ => None,
    }
}
