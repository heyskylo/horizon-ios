//! The client world read off the game thread, for a client mod that bakes from it on its own workers (a minimap's
//! tiles, a world map export, a shader pack's CPU side): the loaded chunks of an area, shared with the world rather
//! than copied, and the biome table that names and tints their biomes.
use super::Game;
use horizon_client::{
    biome::{BiomeColors, Biomes},
    chunk::Chunk,
    options_file::MAX_RENDER_DISTANCE,
    world::World,
};
use horizon_world::registry::BlockStateId;
use std::{ops::Range, sync::Arc};

/// The most chunks a snapshot holds: the square the longest render distance loads, more than a client keeps.
const MAX_CHUNKS: i64 = (2 * MAX_RENDER_DISTANCE as i64 + 1).pow(2);
const CHUNK_SHIFT: i32 = 4;
const CHUNK_MASK: i32 = 15;
const SECTION_HEIGHT: i32 = 16;

/// A biome by the client's own id (vanilla's biomes first, then the world's data packs'), as
/// `NativeClientChunks::biome` reads it. `NativeClientBiomes` names and tints it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeBiomeId(pub u16);

/// The client world's loaded chunks in an area, as they were when `native_client_chunks` took them: `Send` and
/// `Sync`, to read on a worker thread, by block position. It shares the chunks with the world, so taking and
/// cloning one copies no blocks; holding it keeps those chunks as they were, so the world copies the section it
/// edits in a held chunk. Drop the snapshots you no longer read.
#[derive(Clone)]
pub struct NativeClientChunks {
    /// The area's north-west chunk.
    origin: [i32; 2],
    /// The area's width in chunks, along x.
    width: i32,
    /// The area's chunks, a row of `width` after another from north to south; none where not loaded.
    chunks: Vec<Option<Arc<Chunk>>>,
    air: BlockStateId,
    height: Range<i32>,
}

impl NativeClientChunks {
    fn of(world: &World, from: [i32; 2], to: [i32; 2]) -> Result<Self, String> {
        let origin = [from[0].min(to[0]), from[1].min(to[1])];
        let span = |axis: usize| i64::from(from[axis].max(to[axis])) - i64::from(origin[axis]) + 1;
        let (width, depth) = (span(0), span(1));
        let count = width.saturating_mul(depth);
        if count > MAX_CHUNKS {
            return Err(format!("{count} chunks, more than {MAX_CHUNKS} a snapshot"));
        }
        let chunks = (0..depth)
            .flat_map(|dz| (0..width).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| {
                let key = (origin[0] + dx as i32, origin[1] + dz as i32);
                world.chunks.get(&key).cloned()
            })
            .collect();
        Ok(Self {
            origin,
            width: width as i32,
            chunks,
            air: world.air,
            height: world.shape.min_y..world.shape.max_y(),
        })
    }

    #[inline]
    fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        let width = i64::from(self.width);
        let dx = i64::from(x >> CHUNK_SHIFT) - i64::from(self.origin[0]);
        let dz = i64::from(z >> CHUNK_SHIFT) - i64::from(self.origin[1]);
        if !(0..width).contains(&dx) {
            return None;
        }
        self.chunks
            .get(usize::try_from(dz * width + dx).ok()?)?
            .as_deref()
    }

    /// The block state at `pos`: air outside the world's height, and in a column outside the area or not loaded.
    #[inline]
    pub fn block(&self, [x, y, z]: [i32; 3]) -> BlockStateId {
        match self.chunk(x, z) {
            Some(chunk) if self.height.contains(&y) => {
                chunk.block(x & CHUNK_MASK, y, z & CHUNK_MASK)
            }
            _ => self.air,
        }
    }

    /// The biome at `pos`, as the client keeps biomes: one a 4 by 4 by 4 cell, `y` clamped to the world's height.
    /// None in a column outside the area or not loaded.
    #[inline]
    pub fn biome(&self, [x, y, z]: [i32; 3]) -> Option<NativeBiomeId> {
        // The chunks store more than the build height (the Nether's widened to the Overworld's): clamp to it first.
        let y = y.clamp(self.height.start, self.height.end - 1);
        self.chunk(x, z)
            .map(|chunk| NativeBiomeId(chunk.biome(x, y, z)))
    }

    /// Whether the chunk holding column `[x, z]` was loaded (false outside the area).
    #[inline]
    pub fn loaded(&self, [x, z]: [i32; 2]) -> bool {
        self.chunk(x, z).is_some()
    }

    /// One above the highest 16-block section of column `[x, z]`'s chunk that holds anything but air: the column
    /// is air from there up. The world's lowest y where that chunk is all air, or isn't loaded.
    pub fn section_top(&self, [x, z]: [i32; 2]) -> i32 {
        let bottom = self.height.start;
        let Some(chunk) = self.chunk(x, z) else {
            return bottom;
        };
        chunk
            .sections
            .iter()
            .rposition(|section| !section.is_uniform(self.air.0))
            .map_or(bottom, |s| chunk.min_y() + (s as i32 + 1) * SECTION_HEIGHT)
    }

    /// The world's build height: its lowest y, and one above its highest (`native_world_height` gives the lowest y
    /// and the height instead).
    pub fn height(&self) -> Range<i32> {
        self.height.clone()
    }

    fn changed_in(&self, world: &World) -> Vec<[i32; 2]> {
        let width = self.width as usize;
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(i, held)| {
                let pos = [
                    self.origin[0] + (i % width) as i32,
                    self.origin[1] + (i / width) as i32,
                ];
                // A held chunk is shared, so the world copies it before any edit: the same allocation is the same
                // chunk, unchanged.
                let same = match (held, world.chunks.get(&(pos[0], pos[1]))) {
                    (Some(held), Some(now)) => Arc::ptr_eq(held, now),
                    (None, None) => true,
                    _ => false,
                };
                (!same).then_some(pos)
            })
            .collect()
    }
}

/// The client world's biome table, `Send` and `Sync` for a worker thread: each biome's name and tints by
/// `NativeBiomeId`, as the terrain was drawn with them when `native_client_biomes` gave it (a mod's
/// `native_biome_colors` included). An id from another world's snapshot names one of this table's biomes, not
/// necessarily its own.
#[derive(Clone)]
pub struct NativeClientBiomes(Arc<Biomes>);

impl NativeClientBiomes {
    /// The biome's name, `minecraft:plains`.
    pub fn name(&self, biome: NativeBiomeId) -> &str {
        &self.0.get(biome.0).name
    }

    /// The biome's tints at column `[x, z]`, each 0xRRGGBB, before the biome blend the terrain smooths them with:
    /// grass as drawn there (the dark forest's darkening, the swamp's noise), foliage, dry foliage and water.
    pub fn colors(&self, biome: NativeBiomeId, [x, z]: [i32; 2]) -> BiomeColors {
        let info = self.0.get(biome.0);
        BiomeColors {
            grass: self.0.grass(biome.0, f64::from(x), f64::from(z)),
            foliage: info.foliage,
            dry_foliage: info.dry_foliage,
            water: info.water,
        }
    }
}

impl Game<'_> {
    /// For a client mod: the client world's chunks from chunk `from` to chunk `to` (`[x, z]`, both included, in
    /// either order) as they are now, to read on any thread, shared with the world rather than copied. On a client
    /// of a server it is what the server sent. `Err`, taking nothing, on a server, which has no client world, or for
    /// an area of more than 66,049 chunks (the square a render distance of 128 loads). API
    /// @API:client_world_snapshots@.
    pub(super) fn native_client_chunks(
        &self,
        from: [i32; 2],
        to: [i32; 2],
    ) -> Result<NativeClientChunks, String> {
        if !self.native_window() {
            return Err("a server has no client world".into());
        }
        NativeClientChunks::of(&self.world, from, to)
    }

    /// For a client mod: the chunks of `snapshot`'s area (`[x, z]`, in no set order) whose state in the client
    /// world differs from the snapshot's now: written to since it was taken (blocks, biomes or light, even a write
    /// that left them the same), or loaded or unloaded where the snapshot had them the other way. Empty when the
    /// snapshot's blocks and biomes are the world's; their tints are `native_client_biomes_changed`'s. API
    /// @API:client_world_snapshots@.
    pub(super) fn native_client_chunks_changed(
        &self,
        snapshot: &NativeClientChunks,
    ) -> Vec<[i32; 2]> {
        snapshot.changed_in(&self.world)
    }

    /// For a client mod: the biome table the client world's terrain is drawn with, for the ids its snapshots read.
    /// None on a server. API @API:client_world_snapshots@.
    pub(super) fn native_client_biomes(&self) -> Option<NativeClientBiomes> {
        self.native_window()
            .then(|| NativeClientBiomes(self.worker_context().biomes.clone()))
    }

    /// For a client mod: whether the terrain is now drawn with another biome table than `biomes`: a mod recoloured
    /// the biomes (`native_refresh_biome_colors`, a season), the resources loaded again or another world opened.
    /// Then what was tinted from `biomes` is out of date, even where no chunk changed. API
    /// @API:client_world_snapshots@.
    pub(super) fn native_client_biomes_changed(&self, biomes: &NativeClientBiomes) -> bool {
        // `biomes` holds its table, so the game replaces it rather than edit it: the same allocation is the same table.
        !Arc::ptr_eq(&biomes.0, &self.worker_context().biomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockStateId = BlockStateId(1);
    const GLASS: BlockStateId = BlockStateId(2);
    const BIOME: u16 = 7;

    /// An overworld with chunks (0, 0) and (1, 0) loaded, a stone block at y 70 in the first.
    fn world() -> World {
        let mut world = World::new(BlockStateId(0), 0);
        for key in [(0, 0), (1, 0)] {
            world
                .chunks
                .insert(key, Arc::new(Chunk::empty(key.0, key.1)));
        }
        world.set(3, 70, 5, STONE).unwrap();
        world
    }

    #[test]
    fn a_snapshot_reads_the_world_on_another_thread() {
        let mut world = world();
        let chunk = world.chunks.get_mut(&(1, 0)).unwrap();
        Arc::make_mut(chunk).set_biome_cell(0, 0, 0, BIOME);
        let snapshot = NativeClientChunks::of(&world, [1, 0], [-1, 0]).unwrap();
        let read = std::thread::spawn(move || {
            (
                snapshot.block([3, 70, 5]),
                snapshot.block([3, 71, 5]),
                snapshot.loaded([-1, 0]),
                snapshot.loaded([3, 5]),
                snapshot.biome([16, -64, 0]),
                snapshot.biome([-1, 70, 0]),
                snapshot.section_top([3, 5]),
                snapshot.section_top([16, 0]),
                snapshot.height(),
            )
        })
        .join()
        .unwrap();
        assert_eq!(read.0, STONE);
        assert_eq!(read.1, world.air);
        assert!(!read.2, "chunk (-1, 0) is in the area but not loaded");
        assert!(read.3);
        assert_eq!(read.4, Some(NativeBiomeId(BIOME)));
        assert_eq!(read.5, None);
        assert_eq!(read.6, 80, "y 70 is in the section from 64 to 79");
        assert_eq!(read.7, -64, "chunk (1, 0) is all air");
        assert_eq!(read.8, -64..320);
    }

    #[test]
    fn an_edit_changes_only_its_chunk_and_the_snapshot_keeps_its_blocks() {
        let mut world = world();
        let snapshot = NativeClientChunks::of(&world, [0, 0], [1, 0]).unwrap();
        assert!(snapshot.changed_in(&world).is_empty());
        world.set(20, 70, 5, GLASS).unwrap();
        assert_eq!(snapshot.changed_in(&world), vec![[1, 0]]);
        assert_eq!(snapshot.block([20, 70, 5]), world.air);
    }

    #[test]
    fn loading_and_unloading_count_as_changes() {
        let mut world = world();
        let snapshot = NativeClientChunks::of(&world, [-1, 0], [1, 0]).unwrap();
        world.chunks.remove(&(0, 0));
        world.chunks.insert((-1, 0), Arc::new(Chunk::empty(-1, 0)));
        let mut changed = snapshot.changed_in(&world);
        changed.sort();
        assert_eq!(changed, vec![[-1, 0], [0, 0]]);
    }

    #[test]
    fn biomes_read_within_the_build_height() {
        let mut nether = World::new(BlockStateId(0), 0);
        nether.shape = horizon_client::chunk::LevelShape::NETHER;
        let mut chunk = Chunk::empty(0, 0);
        // The Nether's chunks keep the Overworld's 24 sections, from y -64: cell 16 is y 0, cell 79 is y 255.
        for (cell, biome) in [(15, 1), (16, 2), (79, 3), (80, 4)] {
            chunk.set_biome_cell(0, cell, 0, biome);
        }
        nether.chunks.insert((0, 0), Arc::new(chunk));
        let snapshot = NativeClientChunks::of(&nether, [0, 0], [0, 0]).unwrap();
        assert_eq!(snapshot.biome([0, -1, 0]), Some(NativeBiomeId(2)));
        assert_eq!(snapshot.biome([0, i32::MIN, 0]), Some(NativeBiomeId(2)));
        assert_eq!(snapshot.biome([0, 256, 0]), Some(NativeBiomeId(3)));
        assert_eq!(snapshot.biome([0, i32::MAX, 0]), Some(NativeBiomeId(3)));
    }

    #[test]
    fn an_area_past_what_a_client_loads_takes_nothing() {
        let world = world();
        assert!(
            NativeClientChunks::of(&world, [i32::MIN, i32::MIN], [i32::MAX, i32::MAX]).is_err()
        );
        assert!(NativeClientChunks::of(&world, [-128, -128], [128, 128]).is_ok());
    }

    #[test]
    fn a_snapshot_is_send_and_sync() {
        fn shareable<T: Send + Sync>() {}
        shareable::<NativeClientChunks>();
        shareable::<NativeClientBiomes>();
    }
}
