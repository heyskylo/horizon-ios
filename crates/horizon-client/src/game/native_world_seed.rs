//! The world seed for native server mods (docs/native-mods.md, "Server hooks
//! for plugin ports: world generation"): a generator's preview draws the same
//! terrain the chunks will get.
use super::Game;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The world's seed, as `/seed` prints it (`WorldOptions.seed`, Bukkit's
    /// `World.getSeed`): every level of the world generates from it, and
    /// `native_chunk_generate` gets the same value. None off a server: a
    /// client only knows a hash of its server's seed. API @API:world_seed@.
    pub(super) fn native_world_seed(&self) -> Option<i64> {
        self.native_authoritative().then_some(self.level.seed)
    }
}
