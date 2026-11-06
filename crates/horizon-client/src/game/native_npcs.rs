//! NPCs for native server mods (docs/native-mods.md, "Server hooks for plugin
//! ports: NPCs"), what an NPC plugin (Citizens, Sentinel) or a stage plugin
//! needs from the server: spawn into any loaded level, find an entity by its
//! UUID, and drive a mob as vanilla's goals do. They act where the world is
//! simulated, never on a client's copy, and reach any level the server has
//! loaded by its `dimension` (`minecraft:the_nether`), not only the one in
//! place: that level is put in place for the call, as `execute in` does, and
//! the one that was is put back. A level nobody opened is not opened.
use super::Game;

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
