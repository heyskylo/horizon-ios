//! Bulk block edits for server mods: a helper on the level being ticked, as `native_set_block` is, that makes many
//! runs of blocks up columns at once, straight into the chunks.
use super::{
    Game,
    native_claimed_items::{NativeHit, RAYCAST_MAX},
};
use horizon_world::nbt::Tag;
use horizon_world::registry::{BlockRegistry, BlockStateId};

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
/// by index. `neighbour_updates` tells the neighbours of each block changed, as `set_block` does with its flags
/// (API @API:edit_blocks_neighbour_updates@); off, none is told.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NativeBlockEdit {
    pub palette: Vec<String>,
    pub filters: Vec<String>,
    pub runs: Vec<NativeBlockRun>,
    pub neighbour_updates: bool,
    /// Block entity data, loaded after all runs: an entry whose position holds no block entity, or whose `id`
    /// names another type, is skipped (the block stays as the runs made it). API @API:edit_blocks_nbt@.
    pub block_entities: Vec<NativeBlockNbt>,
}

impl Game<'_> {
    /// A block state written at `data_version`, upgraded as the datafixers do it
    /// (`horizon_world::datafix::native_upgrade_block_state`). API @API:upgrade_block_state@.
    pub(super) fn native_upgrade_block_state(
        &self,
        name: &str,
        props: &[(String, String)],
        data_version: i32,
    ) -> (String, Vec<(String, String)>) {
        horizon_world::datafix::native_upgrade_block_state(name, props, data_version)
    }

    /// For a server mod: the runs made at once, straight into the chunks of the authoritative level `dimension`
    /// (the one being ticked, as for `native_set_block`), with what `fill ... strict` does for block entities (a
    /// block replaced by another loses its block entity, and a new block gets the one `native_set_block` would
    /// give it, a mod block's included); no neighbour is told unless `neighbour_updates`, and the chunks changed
    /// go to the players whole.
    /// Runs in chunks not loaded, and the parts of runs outside the column, are left out. The blocks changed, or
    /// why the edit was left out, with nothing changed: a level the server hasn't loaded, a client of a server, a
    /// palette entry or filter that doesn't read or carries NBT (refused), a run naming a palette entry or filter
    /// past the end, or more than 65536 filters. API 126. Since API @API:edit_blocks_any_level@, `dimension` may be
    /// any level the server has loaded: that level is put in place for the edit (`execute in <dimension> run`),
    /// as `native_summon` does, and the one that was is put back.
    pub(super) fn native_edit_blocks(
        &mut self,
        dimension: &str,
        edit: &NativeBlockEdit,
    ) -> Result<u64, String> {
        if dimension != self.dim.id() {
            return self
                .npc_in_level(dimension, |g| g.native_edit_blocks(dimension, edit))
                .unwrap_or_else(|| {
                    Err(format!("{dimension} is not a level the server has loaded"))
                });
        }
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
    /// For a server mod about to edit the block at `pos` of `dimension` for
    /// player `player` (`name`): whether the mods allow it. Asks every mod
    /// (`native_can_build`) with `action`, `place` or `break`; any denies it,
    /// and none at all allows it. Any other `action` is refused. The edit
    /// itself is the caller's: `native_edit_blocks` and `native_set_block`
    /// do not ask.
    pub(super) fn native_may_build(
        &self,
        player: u128,
        name: &str,
        dimension: &str,
        pos: [i32; 3],
        action: &str,
    ) -> bool {
        if !matches!(action, "place" | "break") {
            return false;
        }
        let mut cancel = false;
        self.native_can_build(player, name, dimension, pos, action, &mut cancel);
        !cancel
    }
    /// For a server mod: lights the loaded chunks `chunks` (`[x, z]`, block
    /// coordinates `>> 4`) of the level being ticked again, whole, from their
    /// blocks, and the loaded chunks beside them, as the lighting of
    /// `native_edit_blocks` does but at once, so the light is there before the
    /// chunks are sent (`native_resend_chunk`). A chunk without all eight
    /// neighbours loaded is left out. The light that changed is saved and
    /// goes to the players who see that chunk's border as any relight's does;
    /// the others get it in a resent chunk. Returns how many chunks were lit
    /// (the ring included); 0 for another dimension, on a client. Lights every
    /// chunk it is given: call it for the chunks an edit changed, not every
    /// tick. API @API:chunk_relight_resend@.
    pub(super) fn native_relight_chunks(&mut self, dimension: &str, chunks: &[[i32; 2]]) -> u32 {
        if !self.native_level_open(dimension) {
            return 0;
        }
        let mut columns: Vec<(i32, i32)> = chunks
            .iter()
            .flat_map(|&[x, z]| {
                (-1..=1).flat_map(move |dz| {
                    (-1..=1).map(move |dx| (x.wrapping_add(dx), z.wrapping_add(dz)))
                })
            })
            .filter(|key| self.world.chunks.contains_key(key))
            .collect();
        columns.sort_unstable();
        columns.dedup();
        self.streaming.relight_now(&mut self.world, &columns) as u32
    }

    /// For a server mod: the UUIDs of the players who are sent chunk `chunk`
    /// (`[x, z]`) of the level being ticked and have it, in join order: the
    /// ones to tell of a change there. Empty for another dimension and on a
    /// client. API @API:chunk_relight_resend@.
    pub(super) fn native_chunk_trackers(&self, dimension: &str, chunk: [i32; 2]) -> Vec<u128> {
        if !self.native_level_open(dimension) {
            return Vec::new();
        }
        match &self.lan {
            Some(host) => host.hub.chunk_trackers((chunk[0], chunk[1])),
            None => Vec::new(),
        }
    }

    /// For a server mod: sends chunk `chunk` of the level being ticked again,
    /// whole, with its light, block entities and biomes as they are now, to
    /// every player who has it, or only to `player` when given and it has the
    /// chunk. Returns how many players were sent it; 0 for a chunk not
    /// loaded, a player who does not have it, another dimension, and on a
    /// client. API @API:chunk_relight_resend@.
    pub(super) fn native_resend_chunk(
        &mut self,
        dimension: &str,
        chunk: [i32; 2],
        player: Option<u128>,
    ) -> u32 {
        if !self.native_level_open(dimension) {
            return 0;
        }
        match self.lan.as_mut() {
            Some(host) => {
                host.hub
                    .resend_chunk(&self.world, (chunk[0], chunk[1]), player) as u32
            }
            None => 0,
        }
    }
    /// Whether `dimension` is the authoritative level being ticked: where a server mod's world helpers act.
    pub(super) fn native_level_open(&self, dimension: &str) -> bool {
        (self.serving.is_some() || self.options.dedicated.is_some()) && dimension == self.dim.id()
    }
    /// The data of the block entity at `pos` in the loaded level `dimension`, as `save` writes it without its id
    /// and position (`BlockEntity.saveWithoutMetadata`); None when there is none, or the level isn't loaded (also
    /// on a client). Read-only: a mod that writes one sets the block again. API @API:block_entity_tag@.
    pub(super) fn native_block_entity_tag(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
    ) -> Option<Tag> {
        self.npc_in_level(dimension, |g| g.block_entity_tag(pos))
            .flatten()
    }
    /// The block entity at `pos` in the loaded level `dimension` takes `tag` as its data, as `save` writes it
    /// without its id and position (which stay): what a paste or a schematic restores (a chest's items, a sign's
    /// text) once the block is placed. True when its data changed; false also when it already holds `tag`, when
    /// there is no block entity at `pos`, and on a client. Place the block first (`native_set_block`,
    /// `native_edit_blocks`). API @API:set_block_tag@.
    pub(super) fn native_set_block_tag(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        tag: Tag,
    ) -> bool {
        if dimension != self.dim.id() {
            return self
                .npc_in_level(dimension, |g| g.native_set_block_tag(dimension, pos, tag))
                .unwrap_or(false);
        }
        if !self.native_level_open(dimension) || !matches!(tag, Tag::Compound(_)) {
            return false;
        }
        let snbt = horizon_client::snbt::to_snbt(&tag);
        self.load_block_entity_nbt(pos, &snbt)
    }
    /// The first block a ray from `eye` along `dir` meets in the level being ticked, within `range` (at most
    /// `RAYCAST_MAX`), as the player's own view reads it: the block outline it crosses; with `fluids`, a fluid
    /// block counts too, at its near face (`ClipContext.Fluid.ANY`). `dir` need not be a unit vector. None on a
    /// client, for a zero `dir`, and with no hit in range. API @API:pick_block@.
    pub(super) fn native_pick_block(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        range: f64,
        fluids: bool,
    ) -> Option<NativeHit> {
        if self.serving.is_none() && self.options.dedicated.is_none() {
            return None;
        }
        let len = dir.iter().map(|v| v * v).sum::<f64>().sqrt();
        if len == 0.0 || !len.is_finite() {
            return None;
        }
        let dir = dir.map(|v| v / len);
        let reach = range.clamp(0.0, f64::from(RAYCAST_MAX));
        let hit = self.clip_ray_fluids(eye, dir, reach, fluids)?;
        let [x, y, z] = hit.pos;
        let state = self.world.block(x, y, z);
        Some(NativeHit {
            block_x: x,
            block_y: y,
            block_z: z,
            x: hit.point[0],
            y: hit.point[1],
            z: hit.point[2],
            block: self.res.facts().get(state).name.to_string(),
        })
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
        self.native_level_open(dimension)
            && super::block_edits::parse_block_entity_nbt(nbt)
                .is_ok_and(|tag| self.set_block_entity_checked(pos, tag))
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
}

/// `state` with its property `key` set to `value`; none when the block has
/// no such property or value.
pub(super) fn with_property(
    registry: &BlockRegistry,
    state: BlockStateId,
    key: &str,
    value: &str,
) -> Option<BlockStateId> {
    let mut s = registry.state(state)?.clone();
    match s.properties.get(key) {
        Some(v) if v == value => return Some(state),
        Some(_) => {}
        None => return None,
    }
    s.properties.insert(key.into(), value.into());
    registry.resolve(&s).ok()
}

/// The state a mod's property write changes `state` to; none when the block
/// has no such property or value, or already has it: `LevelChunk.setBlockState`
/// returns null for the same state, so `Level.setBlock` returns false.
pub(super) fn property_change(
    registry: &BlockRegistry,
    state: BlockStateId,
    key: &str,
    value: &str,
) -> Option<BlockStateId> {
    with_property(registry, state, key, value).filter(|&id| id != state)
}
