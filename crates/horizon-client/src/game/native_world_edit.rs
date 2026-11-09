//! Bulk block edits for server mods: a helper on the level being ticked, as `native_set_block` is, that makes many
//! runs of blocks up columns at once, straight into the chunks.
use super::Game;

/// Which of a column's heights (`Heightmap.Types`) a run's `y` counts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHeightmap {
    WorldSurface,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
}
/// A run: `height` blocks up a column from `y` (from the column's `surface` height when one is
/// given), set to palette entry `block`, only where filter `only` matches when one is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeBlockRun {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub height: u32,
    pub surface: Option<NativeHeightmap>,
    pub block: u32,
    pub only: Option<u32>,
}
/// Block entity data for the block at a position once the runs are made, as `/setblock ... {nbt}` loads it (SNBT).
#[derive(Clone, Debug, PartialEq)]
pub struct NativeBlockNbt {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub nbt: String,
}
/// A bulk edit: block states and block predicates as commands write them (without NBT), and the runs naming them
/// by index.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NativeBlockEdit {
    pub palette: Vec<String>,
    pub filters: Vec<String>,
    pub runs: Vec<NativeBlockRun>,
    /// Block entity data, loaded after all runs: an entry whose position holds no block entity, or whose `id`
    /// names another type, is skipped (the block stays as the runs made it).
    pub block_entities: Vec<NativeBlockNbt>,
}

impl Game<'_> {
    /// For a server mod: the runs made at once, straight into the chunks of the authoritative level `dimension`
    /// (the one being ticked, as for `native_set_block`), with what `fill ... strict` does for block entities (a
    /// block replaced by another loses its block entity, and a new block gets the one `native_set_block` would
    /// give it, a mod block's included); no neighbour is told, and the chunks changed go to the players whole.
    /// Runs in chunks not loaded, and the parts of runs outside the column, are left out. The blocks changed, or
    /// why the edit was left out, with nothing changed: another dimension, a client of a server, a palette entry
    /// or filter that doesn't read or carries NBT (refused), a run naming a palette entry or filter past the end,
    /// or more than 65536 filters. API 126.
    pub(super) fn native_edit_blocks(
        &mut self,
        dimension: &str,
        edit: &NativeBlockEdit,
    ) -> Result<u64, String> {
        if !self.native_level_open(dimension) {
            return Err(format!("{dimension} is not the level being ticked"));
        }
        let started = std::time::Instant::now();
        let changed = self.edit_blocks(edit)?;
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!(
                "Horizon: slow mod block edit {ms:.1} ms ({} runs, {changed} blocks)",
                edit.runs.len()
            );
        }
        Ok(changed)
    }
    /// For a server mod: load SNBT `nbt` into the block entity at `pos` of the level being ticked, as the NBT of
    /// `/setblock ... {nbt}` is loaded (`id`, `x`, `y`, `z` are the block entity's own and kept). False, changing
    /// nothing: another dimension, a client, a chunk not loaded, NBT that doesn't read or isn't a compound, no block
    /// entity at `pos`, or an `id` that isn't that block entity's type. API @API:edit_blocks_nbt@.
    pub(super) fn native_set_block_entity(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        nbt: &str,
    ) -> bool {
        self.native_level_open(dimension) && self.set_block_entity_checked(pos, nbt)
    }
    /// For a server mod: the full saved NBT of the block entity at `pos` of the level being ticked, as SNBT
    /// (`/data get block`), None where there is none, in another dimension or on a client. API
    /// @API:edit_blocks_nbt@.
    pub(super) fn native_block_entity_nbt(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
    ) -> Option<String> {
        if !self.native_level_open(dimension) {
            return None;
        }
        self.block_entity_nbt(pos)
            .map(|tag| horizon_client::snbt::to_snbt(&tag))
    }
    /// Whether `dimension` is the authoritative level being ticked: where a server mod's world helpers act.
    pub(super) fn native_level_open(&self, dimension: &str) -> bool {
        (self.serving.is_some() || self.options.dedicated.is_some()) && dimension == self.dim.id()
    }
    /// The entities of the loaded level `dimension` inside the box from `min` to `max` (both corners included), by
    /// id, as `native_level_entities` lists them; players the hub holds are included. Empty for a level not loaded,
    /// and on a client. Walks every entity of the level: a mod calls it for a few boxes, not every tick. API
    /// @API:entities_in@.
    pub(super) fn native_entities_in(
        &mut self,
        dimension: &str,
        min: [f64; 3],
        max: [f64; 3],
    ) -> Vec<u32> {
        self.native_level_entities(dimension)
            .into_iter()
            .filter(|(_, _, p)| (0..3).all(|i| min[i] <= p[i] && p[i] <= max[i]))
            .map(|(id, _, _)| id)
            .collect()
    }
}
