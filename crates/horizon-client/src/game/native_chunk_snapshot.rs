//! Read-only chunk snapshots for native server mods (docs/native-mods.md,
//! "Server hooks for plugin ports: world"): a web map or a backup tool takes
//! a loaded chunk as it is now and reads it later, off the server thread,
//! with the block map colours and biome tints to draw it.
use super::{Dim, Game};
use horizon_client::{biome::BiomeColors, blocks::MAP_COLORS, chunk::Chunk};
use horizon_core::{biome::BiomeZoom, position::BlockPos};
use horizon_world::registry::BlockStateId;
use std::sync::Arc;

/// A loaded chunk as it was when `native_chunk_snapshot` took it: its block
/// states and biomes, shared with the level until the level changes them
/// (copy on write), so taking one is cheap and holding it doesn't stop the
/// level. It can be sent to another thread. Coordinates are the level's
/// block coordinates.
#[derive(Clone)]
pub struct NativeChunkSnapshot {
    chunk: Arc<Chunk>,
    pos: [i32; 2],
    min_y: i32,
    top_y: i32,
    ceiling: bool,
    zoom: BiomeZoom,
    /// Air, cave air and void air (`BlockState.isAir`).
    air: [Option<BlockStateId>; 3],
}

impl NativeChunkSnapshot {
    /// The chunk's position, `[x, z]` (block coordinates `>> 4`).
    pub fn chunk(&self) -> [i32; 2] {
        self.pos
    }
    /// The level's lowest block `y` and one above its highest
    /// (`getMinY`, `getMaxY` + 1).
    pub fn height(&self) -> (i32, i32) {
        (self.min_y, self.top_y)
    }
    /// Whether the level has a ceiling (`DimensionType.hasCeiling`: the Nether).
    pub fn has_ceiling(&self) -> bool {
        self.ceiling
    }
    /// The block state at `x, y, z` (`getBlockState`), as the state helpers
    /// take it (`native_state_block`); None outside this chunk or the level's
    /// height.
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockStateId> {
        let inside =
            (x >> 4, z >> 4) == (self.pos[0], self.pos[1]) && (self.min_y..self.top_y).contains(&y);
        inside.then(|| self.chunk.block(x & 15, y, z & 15))
    }
    /// `getHeight(Heightmap.Types.WORLD_SURFACE, x, z)`: one above the
    /// highest block that isn't air, the level's lowest `y` in an empty
    /// column. None outside this chunk.
    pub fn surface(&self, x: i32, z: i32) -> Option<i32> {
        let air = self.air;
        ((x >> 4, z >> 4) == (self.pos[0], self.pos[1])).then(|| {
            self.chunk
                .world_surface(x & 15, z & 15, |v| air.contains(&Some(BlockStateId(v))))
        })
    }
    /// The biome of quart cell `qx, qy, qz` (block coordinates `>> 2`,
    /// `getNoiseBiome`), the `y` clamped to the level, as a biome id of
    /// `native_biome_name` and `native_biome_tint`. None for a cell outside
    /// this chunk.
    pub fn noise_biome(&self, qx: i32, qy: i32, qz: i32) -> Option<u16> {
        let (x, z) = (qx << 2, qz << 2);
        ((x >> 4, z >> 4) == (self.pos[0], self.pos[1])).then(|| self.chunk.biome(x, qy << 2, z))
    }
    /// The quart cell whose biome is the biome at block `x, y, z`
    /// (`BiomeManager.getBiome`'s fuzzy zoom with the world's seed): it can
    /// lie in a neighbouring chunk, so read it with `noise_biome` of that
    /// chunk's snapshot.
    pub fn biome_cell(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.zoom.quart_at_block(BlockPos::new(x, y, z))
    }
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// A snapshot of loaded chunk `chunk` (`[x, z]`) of the level `dimension`
    /// (`minecraft:the_nether`), in place or open with nobody in it. None off
    /// a server, for a level not open and for a chunk not loaded; nothing is
    /// loaded or generated for it. API @API:chunk_snapshot@.
    pub(super) fn native_chunk_snapshot(
        &self,
        dimension: &str,
        chunk: [i32; 2],
    ) -> Option<NativeChunkSnapshot> {
        if !self.native_authoritative() {
            return None;
        }
        let world = if dimension == self.dim.id() {
            &self.world
        } else {
            let dim = [Dim::Overworld, Dim::Nether, Dim::End]
                .into_iter()
                .find(|d| d.id() == dimension)?;
            self.levels.parked_world(dim)?
        };
        Some(NativeChunkSnapshot {
            chunk: world.chunks.get(&(chunk[0], chunk[1]))?.clone(),
            pos: chunk,
            min_y: world.shape.min_y,
            top_y: world.shape.max_y(),
            ceiling: world.shape.has_ceiling,
            zoom: self.streaming.biome_zoom,
            air: ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"]
                .map(|name| self.pack.assets.registry.default_state(name)),
        })
    }

    /// The map colour of block state `state` (`BlockState.getMapColor`):
    /// `(id, colour)`, the `MapColor` id and its `col` (0xRRGGBB), `(0, 0)`
    /// for `MapColor.NONE` (air, glass). None for an unknown state. API
    /// @API:chunk_snapshot@.
    pub(super) fn native_block_map_color(&self, state: BlockStateId) -> Option<(u8, u32)> {
        let known = usize::try_from(state.0).is_ok_and(|s| s < self.pack.assets.registry.len());
        if !known {
            return None;
        }
        let id = self.res.facts().get(state).map_color;
        Some((id, MAP_COLORS.get(usize::from(id)).copied().unwrap_or(0)))
    }

    /// The name of biome id `biome` (`minecraft:plains`), as a snapshot's
    /// `noise_biome` gives it. None for an unknown id. API
    /// @API:chunk_snapshot@.
    pub(super) fn native_biome_name(&self, biome: u16) -> Option<&str> {
        let biomes = world_biomes!(self);
        biomes.list.get(usize::from(biome)).map(|b| b.name.as_str())
    }

    /// The tints of biome id `biome` at block column `x, z`, each 0xRRGGBB:
    /// grass (`Biome.getGrassColor`, with the dark forest's and the swamp's
    /// modifiers), foliage, dry foliage and water, after
    /// `native_biome_colors`. The biome's own, not blended over the biome
    /// blend radius as the client draws them. None for an unknown id. API
    /// @API:chunk_snapshot@.
    pub(super) fn native_biome_tint(&self, biome: u16, x: i32, z: i32) -> Option<BiomeColors> {
        let biomes = world_biomes!(self);
        let b = biomes.list.get(usize::from(biome))?;
        Some(BiomeColors {
            grass: biomes.grass(biome, f64::from(x), f64::from(z)),
            foliage: b.foliage,
            dry_foliage: b.dry_foliage,
            water: b.water,
        })
    }
}
