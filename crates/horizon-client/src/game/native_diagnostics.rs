//! What a server's diagnostics mod reads (docs/native-mods.md, "Server hooks
//! for plugin ports: diagnostics"): the tick clock as vanilla and Bukkit
//! report it, what each open level holds, the packets each player's
//! connection carries, and the switches of the tick's profiler (spans and
//! sampled mob costs). Reads only; nothing here changes what the server
//! does.
use super::{Dim, Game, commands::Target, native_entities::target_id, tick_profile};
use horizon_client::{
    mod_api::{NativeLevelStats, NativeMobCost},
    world::World,
};
use std::collections::BTreeMap;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The last 100 server ticks' durations in nanoseconds, as vanilla's
    /// `MinecraftServer.getTickTimesNanos` lays them out: the tick numbered
    /// `n` at index `n % 100` (0 where none was recorded yet). Sort a copy
    /// for percentiles; the average is `native_tick_stats`'. On a client, its
    /// own ticks. API @API:tick_times@.
    pub(super) fn native_tick_times(&self) -> Vec<u64> {
        self.clock.tick_times_nanos()
    }
    /// The ticks a second over the last one, five and fifteen minutes
    /// (Bukkit's `Server.getTPS`), measured between the ends of the recorded
    /// ticks and averaged once a second as a load average is; 20 at the
    /// start. A sprint reads more than the rate, a server that falls behind
    /// less. API @API:tick_times@.
    pub(super) fn native_tps(&self) -> [f64; 3] {
        self.clock.tps()
    }
    /// The counts of the open level `dimension` (`minecraft:the_nether`):
    /// its loaded and forced chunks, its block entities (and how many tick),
    /// and its entities and block entities by type. A level not in place is
    /// swapped in for the count and back, as `/execute in` does; the players
    /// are those the hub holds, so inside a player's own hook (the hub taken
    /// out) none are counted. A walk over every entity: once a second or
    /// less, not every tick. None on a client and for a level that isn't
    /// open. API @API:level_stats@.
    pub(super) fn native_level_stats(&mut self, dimension: &str) -> Option<NativeLevelStats> {
        let dim = self.diagnosed_level(dimension)?;
        self.with_level_in_place(dim, |game| {
            let mut entities = BTreeMap::new();
            for (_, kind, _) in game.entities_in_place() {
                *entities.entry(kind).or_default() += 1;
            }
            let mut block_entity_types = BTreeMap::new();
            for (_, kind) in game.block_entity_types(&game.world) {
                *block_entity_types.entry(kind).or_default() += 1;
            }
            NativeLevelStats {
                chunks: game.world.chunks.len(),
                forced_chunks: game.streaming.forced.len(),
                block_entities: game.world.block_entities.len(),
                ticking_block_entities: game.world.block_entities.ticking_len(),
                entities: most_first(entities),
                block_entity_types: most_first(block_entity_types),
            }
        })
    }
    /// Every entity of the open level `dimension`: its id (the one events
    /// and `native_entity` take), its type and its position, players
    /// included (as `native_level_stats` counts them). Empty on a client and
    /// for a level that isn't open. API @API:level_stats@.
    pub(super) fn native_level_entities(
        &mut self,
        dimension: &str,
    ) -> Vec<(u32, String, [f64; 3])> {
        let Some(dim) = self.diagnosed_level(dimension) else {
            return Vec::new();
        };
        self.with_level_in_place(dim, Self::entities_in_place)
            .unwrap_or_default()
    }
    /// Every block entity of the open level `dimension`: its position and
    /// its type, the vanilla block entity type of its block (`minecraft:chest`,
    /// `minecraft:barrel`, `minecraft:mob_spawner`), else the block's name (a
    /// mod's). Empty on a client and for a level that isn't open. API
    /// @API:level_stats@.
    pub(super) fn native_level_block_entities(&self, dimension: &str) -> Vec<([i32; 3], String)> {
        let world = match self.diagnosed_level(dimension) {
            Some(dim) if dim == self.dim => Some(&self.world),
            Some(dim) => self.levels.parked_world(dim),
            None => None,
        };
        world.map_or_else(Vec::new, |world| self.block_entity_types(world))
    }

    /// Keep (or stop keeping) every player's play packets for
    /// `native_server_packets`, from now on: while on, each packet costs the
    /// network thread a lock and a list entry, so turn it on for a session
    /// and off after. Off drops what is kept. A switch for the whole server,
    /// shared by every mod. API @API:server_packets@.
    pub(super) fn native_count_packets(&self, on: bool) {
        horizon_client::lan::conn::count_packets(on);
        if !on {
            for g in self.lan.iter().flat_map(|h| h.hub.guests.iter()) {
                g.sender.take_packets();
            }
        }
    }
    /// The end of a server tick: each player's packets kept since the last
    /// one go to `native_server_packets`. One relaxed load while nobody counts.
    pub(super) fn diagnostics_packets_tick(&mut self) {
        if !horizon_client::lan::conn::counting_packets() {
            return;
        }
        let kept: Vec<_> = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .map(|g| (g.uuid, g.sender.take_packets()))
            .collect();
        for (player, [sent, received]) in kept {
            if !sent.is_empty() {
                self.native_server_packets(player, &sent, true);
            }
            if !received.is_empty() {
                self.native_server_packets(player, &received, false);
            }
        }
    }

    /// Record the server tick's spans for `native_server_tick_timed` (or no
    /// longer): elsewhere than on a dedicated server, which records them
    /// anyway, the hook fires only while they are on. One switch shared with
    /// `HORIZON_SPIKES`, performance reports and every mod: spans stay on
    /// while anyone wants them. Spans are recorded on every thread (two clock
    /// reads a step): turn them on for a profiling session and off after.
    /// Only on a server. API @API:server_tick_timed@.
    pub(super) fn native_spans(&self, on: bool) {
        if self.native_authoritative() {
            tick_profile::want_spans(tick_profile::SPANS_MOD, on);
        }
    }
    /// Time every mob's own tick (its riders inside it) from now on, or no
    /// longer, summed by level and mob type for `native_take_mob_costs`:
    /// two clock reads and a map update a mob while on, one thread-local
    /// read off. Sampled apart from a performance report's, so both can run.
    /// For a lighter profile, turn it on in `ServerTickStart` of one tick in
    /// N and off after. Call it from a server hook (it belongs to the server
    /// thread). API @API:server_tick_timed@.
    pub(super) fn native_mob_cost_sampling(&self, on: bool) {
        tick_profile::set_mod_sampling(on);
    }
    /// The mob costs sampled since the last call (`native_mob_cost_sampling`),
    /// costliest first; clears them. API @API:server_tick_timed@.
    pub(super) fn native_take_mob_costs(&self) -> Vec<NativeMobCost> {
        tick_profile::take_mod()
            .into_iter()
            .map(|c| NativeMobCost {
                dimension: c.dimension.to_owned(),
                kind: c.kind,
                nanos: c.nanos,
                mob_ticks: c.mob_ticks,
            })
            .collect()
    }

    /// The open level `dimension` names, on a server.
    fn diagnosed_level(&self, dimension: &str) -> Option<Dim> {
        if !self.native_authoritative() {
            return None;
        }
        std::iter::once(self.dim)
            .chain(self.levels.parked_dims())
            .find(|d| d.id() == dimension)
    }
    /// `f` with the level `dim` in place, then the level that was in place
    /// back (`/execute in`'s swap). None when it could not be opened.
    fn with_level_in_place<R>(&mut self, dim: Dim, f: impl FnOnce(&Self) -> R) -> Option<R> {
        let back = self.enter_command_level(dim);
        let result = (self.dim == dim).then(|| f(self));
        if let Some(back) = back {
            self.enter_command_level(back);
        }
        result
    }
    /// The entities of the level in place, as a selector finds them (`@e`),
    /// and the hub's players in it.
    fn entities_in_place(&self) -> Vec<(u32, String, [f64; 3])> {
        let mut out = Vec::new();
        for t in self.level_candidates() {
            if let Target::Mob(id) = t
                && !self.mobs.iter().any(|m| m.id == id && !m.removed)
            {
                continue;
            }
            if let (Some(id), Some(pos)) = (target_id(t), self.target_position(t)) {
                out.push((id, self.target_type(t), pos));
            }
        }
        let dimension = self.dim.id();
        for g in self.lan.iter().flat_map(|h| h.hub.guests.iter()) {
            if g.placed && g.dimension == dimension {
                out.push((
                    g.entity_id as u32,
                    "minecraft:player".to_owned(),
                    g.position,
                ));
            }
        }
        out
    }
    /// A world's block entities with their types (`native_level_block_entities`).
    fn block_entity_types(&self, world: &World) -> Vec<([i32; 3], String)> {
        let facts = self.res.facts();
        world
            .block_entities
            .iter()
            .map(|(&(x, y, z), _)| {
                let name = &facts.get(world.block(x, y, z)).name;
                let kind = horizon_client::lan::chunks::block_entity_type(name)
                    .map_or_else(|| name.to_string(), str::to_owned);
                ([x, y, z], kind)
            })
            .collect()
    }
}

/// Counts by name, the most first and ties by name.
fn most_first(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut out: Vec<_> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    /// Most first; a tie keeps the names in order.
    #[test]
    fn counts_come_most_first() {
        let counts = [
            ("minecraft:cow", 2),
            ("minecraft:bat", 2),
            ("minecraft:zombie", 5),
        ]
        .map(|(k, n)| (k.to_owned(), n))
        .into_iter()
        .collect();
        let names: Vec<_> = super::most_first(counts)
            .into_iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        assert_eq!(
            names,
            ["minecraft:zombie 5", "minecraft:bat 2", "minecraft:cow 2"]
        );
    }
}
