//! The world border for native server mods (docs/native-mods.md, "Server
//! hooks for plugin ports: world"): a web map draws it, a pre-generator
//! fills it.
use super::Game;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The world border of the level `dimension` (`minecraft:the_nether`) as
    /// the server enforces it: `[min_x, min_z, max_x, max_z]` in blocks
    /// (`WorldBorder.getMinX` and the others, the previous size while it
    /// moves). Vanilla keeps a border per level (`ServerLevel.getWorldBorder`,
    /// each level's `data/minecraft/world_border.dat`); Horizon keeps one for
    /// every level today, so the three levels answer the same bounds, and
    /// this follows when that changes. None off a server and for a dimension
    /// other than the Overworld, the Nether and the End. API
    /// @API:world_border@.
    pub(super) fn native_world_border(&self, dimension: &str) -> Option<[f64; 4]> {
        let known = matches!(
            dimension,
            "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
        );
        (known && self.native_authoritative()).then(|| self.border.bounds())
    }
}
