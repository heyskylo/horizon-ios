//! NPCs for native server mods (docs/native-mods.md, "Server hooks for plugin
//! ports: NPCs"), what an NPC plugin (Citizens, Sentinel) or a stage plugin
//! needs from the server: spawn into any loaded level, find an entity by its
//! UUID, and drive a mob as vanilla's goals do. They act where the world is
//! simulated, never on a client's copy, and reach any level the server has
//! loaded by its `dimension` (`minecraft:the_nether`), not only the one in
//! place: that level is put in place for the call, as `execute in` does, and
//! the one that was is put back. A level nobody opened is not opened.
use super::{Game, commands::Target};
use horizon_world::nbt::Compound;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The entity with `uuid` in any loaded level (Bukkit's
    /// `Server.getEntity(UUID)`, `ServerLevel.getEntity(UUID)` over every
    /// level): its dimension, its id there and its type (`minecraft:zombie`).
    /// A server's player is found too (`minecraft:player`, in its level). None
    /// on a client, for no such entity, and for a player while a guest's
    /// packet is handled (the hub is taken out then): call it from a tick or
    /// console hook. API @API:npc_entity_by_uuid@.
    pub(super) fn native_entity_by_uuid(&mut self, uuid: u128) -> Option<(String, u32, String)> {
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return None;
        }
        let player = self.lan.as_ref().and_then(|h| {
            let g = h.hub.guests.iter().find(|g| g.placed && g.uuid == uuid)?;
            Some((g.dimension.clone(), u32::try_from(g.entity_id).ok()?))
        });
        if let Some((dimension, id)) = player {
            return Some((dimension, id, "minecraft:player".into()));
        }
        let words = horizon_client::lan::playerdata::uuid_ints(uuid);
        let dims: Vec<&'static str> = std::iter::once(self.dim)
            .chain(self.levels.parked_dims())
            .map(|d| d.id())
            .collect();
        for dimension in dims {
            let found = self.npc_in_level(dimension, |g| {
                // The UUIDs the level remembers, checked against its entities: a
                // remembered id may belong to an entity that is gone.
                let id = g
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, w)| **w == words)
                    .map(|(id, _)| *id)?;
                let t = g.level_entity(i32::try_from(id).ok()?)?;
                if let Target::Mob(m) = t
                    && !g.mobs.iter().any(|x| x.id == m && !x.removed)
                {
                    return None;
                }
                Some((id, g.target_type(t)))
            });
            if let Some(Some((id, kind))) = found {
                return Some((dimension.to_owned(), id, kind));
            }
        }
        None
    }
    /// Merges `nbt` into entity `id`'s data in the loaded level `dimension`,
    /// as `/execute in <dimension> run data merge entity` does: what
    /// `native_entity_merge_nbt` does to the level in place, for any loaded
    /// level (with `native_entity_by_uuid`, an entity found by its UUID).
    /// False on a client, for a level not loaded, no such entity, or what
    /// `native_entity_merge_nbt` refuses (a player, a kind Horizon can't
    /// reload yet, data that changes nothing). API @API:npc_entity_by_uuid@.
    pub(super) fn native_merge_entity_nbt(
        &mut self,
        dimension: &str,
        id: u32,
        nbt: Compound,
    ) -> bool {
        self.npc_in_level(dimension, |g| {
            g.native_entity(id)
                .is_some_and(|e| g.native_entity_merge_nbt(e, nbt))
        })
        .unwrap_or(false)
    }
}

impl Game<'_> {
    /// Runs `f` with the loaded level `dimension` in place, then puts back
    /// the level that was (`execute in <dimension> run ...`): None on a
    /// client, or for a dimension the server hasn't loaded. The level in place
    /// runs `f` as it is. Not a helper of its own (a closure isn't one): the
    /// helpers that reach any loaded level go through it.
    pub(super) fn npc_in_level<R>(
        &mut self,
        dimension: &str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return None;
        }
        if dimension == self.dim.id() {
            return Some(f(self));
        }
        let dim = self
            .levels
            .parked_dims()
            .into_iter()
            .find(|d| d.id() == dimension)?;
        let back = self.enter_command_level(dim)?;
        let out = f(self);
        self.enter_command_level(back);
        Some(out)
    }
}
