//! Chunks a native mod keeps loaded (NeoForge's `TicketController`): a train
//! running where nobody is, a long contraption, a machine that must keep
//! working (docs/native-mods.md, "Chunk tickets"). A held chunk is as a
//! `/forceload`ed one, its level stays open, but `/forceload` neither lists
//! nor releases it and it isn't saved.
use super::Game;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
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
    /// set of owners changed; false for an owner without a namespace and off
    /// a server. API @API:chunk_tickets@.
    pub(super) fn native_force_chunk(&mut self, owner: &str, chunk: [i32; 2], hold: bool) -> bool {
        let namespaced = owner
            .split_once(':')
            .is_some_and(|(ns, path)| !ns.is_empty() && !path.is_empty());
        if !namespaced || !self.native_authoritative() {
            return false;
        }
        let key = (chunk[0], chunk[1]);
        let level = self.mod_chunk_tickets.entry(self.dim).or_default();
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
        if level.contains_key(&key) {
            if self.streaming.mod_forced.insert(key) {
                self.streaming.rank_forced(key.0, key.1, key.0, key.1);
            }
        } else {
            self.streaming.mod_forced.remove(&key);
        }
        true
    }

    /// The chunks of the level in place `owner` holds (`native_force_chunk`),
    /// sorted. API @API:chunk_tickets@.
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
