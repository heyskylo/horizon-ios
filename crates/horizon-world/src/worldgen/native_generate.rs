//! Terrain of a native mod's own (docs/native-mods.md, "Server hooks for
//! plugin ports: world generation"): a chunk a mod builds itself where
//! vanilla's noise would fill it, as Bukkit's `ChunkGenerator` does.
use super::{
    biome::BiomeId,
    biome_chunk::BiomeChunk,
    generator::{GenBlockStates, GeneratorData, SurfaceSample},
    generator_structures::Starts,
    structure::start::StructureStart,
    terrain::TerrainChunk,
};
use crate::{
    heightmap::HeightmapState,
    registry::{BlockRegistry, BlockStateId},
};
use std::sync::{Arc, OnceLock};

/// What a native generator made of a chunk (`native_chunk_generate`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeGenerate {
    /// Vanilla's terrain: the noise (or a superflat world's layers), the
    /// aquifers, the surface rule and the carvers.
    #[default]
    Vanilla,
    /// The mod set the chunk's blocks; vanilla adds what the flags ask for
    /// (Bukkit's `ChunkGenerator.shouldGenerateSurface`, `shouldGenerateCaves`,
    /// `shouldGenerateStructures` and `shouldGenerateDecorations`).
    Replaced {
        /// The dimension's surface rule paints the mod's terrain (grass over
        /// dirt, sand on beaches), as it paints the noise's.
        surface: bool,
        /// Vanilla's carvers cut their caves and canyons into it.
        carvers: bool,
        /// The chunk keeps its structure starts and references; false is
        /// [`NativeProtoChunk::clear_structures`].
        structures: bool,
        /// Decoration places the biomes' features in it (ores, trees, grass,
        /// lakes); structure pieces are placed either way.
        decorations: bool,
    },
}

/// A chunk being generated, as `native_chunk_generate` sees it: its biomes
/// and structure starts are there, its blocks are all air.
pub struct NativeProtoChunk<'a> {
    pub(super) terrain: &'a mut TerrainChunk<'static, GenBlockStates>,
    pub(super) biomes: &'a mut BiomeChunk<BiomeId>,
    pub(super) data: &'a GeneratorData,
    pub(super) references: &'a mut Vec<Arc<StructureStart>>,
    pub(super) own_starts: &'a mut Starts,
}
impl NativeProtoChunk<'_> {
    /// The chunk's position, `[x, z]` in chunks.
    pub fn position(&self) -> [i32; 2] {
        let pos = self.terrain.position();
        [pos.x, pos.z]
    }
    /// The level's build range the chunk spans: its lowest Y and its height.
    pub fn level(&self) -> (i32, i32) {
        (self.terrain.min_y(), self.terrain.height())
    }
    /// The block states (`default_state("minecraft:stone")`, `resolve` for one
    /// with properties).
    pub fn registry(&self) -> &'static BlockRegistry {
        self.data.registry()
    }
    /// Set the block at `x y z` (block coordinates in this chunk) to `state`,
    /// as the noise writes it: no updates, no block entity yet (the chunk gets
    /// its blocks' default ones when it is finished). False outside the chunk
    /// or the level and for an unknown state.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: BlockStateId) -> bool {
        let pos = self.terrain.position();
        x >> 4 == pos.x
            && z >> 4 == pos.z
            && self.data.registry().state(state).is_some()
            && self.terrain.set_block_raw(x, y, z, state).is_ok()
    }
    /// A biome's id (`minecraft:plains`); None for a biome the world lacks.
    pub fn biome_id(&self, biome: &str) -> Option<BiomeId> {
        self.data.biome_id(biome)
    }
    /// Refill the chunk's biomes: `pick` answers each quart (a block position
    /// `>> 2`) its biome, or None to keep vanilla's. Decoration, the saved
    /// chunk and the players see them; the surface rule, structure placement
    /// and the far terrain read the biome source as before. True when a quart
    /// changed.
    pub fn set_biomes(&mut self, mut pick: impl FnMut([i32; 3]) -> Option<BiomeId>) -> bool {
        let known = self.data.biome_names().len();
        let old = self.biomes.clone();
        let (low, high) = old.quart_range();
        let mut changed = false;
        let filled = self.biomes.fill(low << 2, (high << 2) + 3, |quart| {
            let was = *old.noise_biome(quart);
            let now = pick(quart)
                .filter(|id| (id.0 as usize) < known)
                .unwrap_or(was);
            changed |= now != was;
            Ok::<_, std::convert::Infallible>(now)
        });
        filled.is_ok() && changed
    }
    /// Drop the chunk's structure starts and references (Bukkit's
    /// `shouldGenerateStructures` false): decoration places no structure piece
    /// in it. Pieces of a start in this chunk still reach neighbours a mod
    /// leaves vanilla, and `/locate` still finds it.
    pub fn clear_structures(&mut self) {
        self.references.clear();
        *self.own_starts = Starts::default();
    }
}

/// A native mod's hook point (docs/native-mods.md), listed in
/// `native-hooks.json` with the game's: chunk `[x, z]` of the level
/// `dimension` (`minecraft:overworld`) is about to get its terrain, after
/// its biomes and structure starts and before the noise (Bukkit's
/// `ChunkGenerator.generateNoise`; vanilla's `ChunkStatus.NOISE`). `seed` is
/// the world's (`native_world_seed`); `flat` says the level is superflat
/// (its layers stand where the noise would). A mod that builds the chunk
/// sets its blocks with `chunk_out.set_block` (all air until then) and sets
/// `generate` to `Replaced`, whose flags say what vanilla adds: the surface
/// rule, the carvers (on a superflat level too), the structures, the
/// features. Leave `generate` at `Vanilla` and set no block to keep vanilla's
/// terrain; `set_biomes` and `clear_structures` work with either answer. Mods
/// are asked in turn and see what earlier ones set. Called on the generation
/// workers, for several chunks at once on several threads and never on the
/// server thread: keep what the mod reads `Sync`, and no `Game` helper works
/// here. A chunk can be asked again (a proto chunk saved unfinished, the
/// neighbours of a finished chunk, the far terrain's decoration), so answer
/// the same each time. Server-side, and wherever chunks generate (single
/// player, far terrain). @API:chunk_generate@.
#[inline(never)]
pub fn native_chunk_generate(
    dimension: &str,
    chunk: [i32; 2],
    seed: i64,
    flat: bool,
    chunk_out: &mut NativeProtoChunk<'_>,
    generate: &mut NativeGenerate,
) {
    chunk_generate(dimension, chunk, seed, flat, chunk_out, generate);
}

/// A far terrain column, as `native_lod_column` sees it.
pub struct NativeLodColumn<'a> {
    data: &'a GeneratorData,
    replaced: Option<(Vec<(i32, i32, BlockStateId)>, Option<BiomeId>)>,
}
impl NativeLodColumn<'_> {
    /// The block states (`default_state("minecraft:stone")`).
    pub fn registry(&self) -> &'static BlockRegistry {
        self.data.registry()
    }
    /// A biome's id (`minecraft:plains`); None for a biome the world lacks.
    pub fn biome_id(&self, biome: &str) -> Option<BiomeId> {
        self.data.biome_id(biome)
    }
    /// Show the column as the mod's chunk will have it: `spans` are its
    /// blocks, runs `(lowest y, highest y + 1, state)` in any order (air
    /// between them), and `biome` the biome its top shows (None: vanilla's).
    pub fn replace(&mut self, spans: Vec<(i32, i32, BlockStateId)>, biome: Option<BiomeId>) {
        self.replaced = Some((spans, biome));
    }
}

/// A native mod's hook point (docs/native-mods.md), listed in
/// `native-hooks.json` with the game's: the far terrain samples the block
/// column `[x, z]` of the level `dimension` that is not generated yet
/// (vanilla's noise surface, its top state painted by the surface rule).
/// A mod whose `native_chunk_generate` builds that chunk calls
/// `column_out.replace` with the column's blocks, and the far terrain shows
/// those until the chunk is generated (its top and the water over it, the
/// spans of a cliff); left alone, the column is vanilla's. `seed` and
/// `flat` are as for `native_chunk_generate`. Asked for each far column,
/// many per tile, on the far terrain's workers, several at once and never
/// on the server thread: answer from the terrain function alone, quickly,
/// without building the chunk. Wherever far terrain is sampled (a client's
/// own far terrain, single player). @API:lod_column@.
#[inline(never)]
pub fn native_lod_column(
    dimension: &str,
    column: [i32; 2],
    seed: i64,
    flat: bool,
    column_out: &mut NativeLodColumn<'_>,
) {
    lod_column(dimension, column, seed, flat, column_out);
}

/// Callbacks into the executable for the generation hooks, installed once
/// before any level generates, as `native_load_hooks`; without them both hooks
/// do nothing.
pub struct NativeGenerateHooks {
    pub chunk_generate:
        fn(&str, [i32; 2], i64, bool, &mut NativeProtoChunk<'_>, &mut NativeGenerate),
    pub lod_column: fn(&str, [i32; 2], i64, bool, &mut NativeLodColumn<'_>),
}

static HOOKS: OnceLock<NativeGenerateHooks> = OnceLock::new();

/// Install the process-wide callbacks once; a second installation keeps the first.
pub fn install(hooks: NativeGenerateHooks) -> Result<(), NativeGenerateHooks> {
    HOOKS.set(hooks)
}

fn chunk_generate(
    dimension: &str,
    chunk: [i32; 2],
    seed: i64,
    flat: bool,
    chunk_out: &mut NativeProtoChunk<'_>,
    generate: &mut NativeGenerate,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.chunk_generate)(dimension, chunk, seed, flat, chunk_out, generate);
    }
}

fn lod_column(
    dimension: &str,
    column: [i32; 2],
    seed: i64,
    flat: bool,
    column_out: &mut NativeLodColumn<'_>,
) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.lod_column)(dimension, column, seed, flat, column_out);
    }
}

/// Ask `native_lod_column` about the far column `[x, z]` and put what a mod
/// answers in place of vanilla's `sample` and `spans` (top down). Nothing
/// changes when no mod answers.
pub(super) fn far_column(
    (dimension, seed, flat): (&str, i64, bool),
    data: &GeneratorData,
    column: [i32; 2],
    sample: &mut Option<SurfaceSample>,
    spans: Option<&mut Vec<(i32, i32, BlockStateId)>>,
) {
    let mut out = NativeLodColumn {
        data,
        replaced: None,
    };
    native_lod_column(dimension, column, seed, flat, &mut out);
    let Some((mut runs, biome)) = out.replaced else {
        return;
    };
    let states = data.block_states();
    runs.retain(|&(min, max, state)| {
        min < max && data.registry().state(state).is_some() && !states.get(state).is_air()
    });
    runs.sort_by_key(|a| std::cmp::Reverse(a.0));
    // As vanilla's sample reads a column down: the first block without a
    // fluid is its top, and the fluid right over it its water.
    let mut water = None;
    let mut above = None;
    let mut top = None;
    for &(min, max, state) in &runs {
        if above != Some(max) {
            water = None;
        }
        above = Some(min);
        if states.get(state).has_fluid() {
            water.get_or_insert((max, state));
        } else {
            top = Some((max, state));
            break;
        }
    }
    let biome = biome
        .or(sample.as_ref().map(|s| s.biome))
        .unwrap_or(BiomeId(0));
    *sample = top.map(|(height, state)| SurfaceSample {
        height,
        state,
        water,
        biome,
    });
    if let Some(spans) = spans {
        *spans = runs;
    }
}
