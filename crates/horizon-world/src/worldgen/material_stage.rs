//! Surface-stage column traversal and pre-lighting chunk writes.
use super::{
    DecodeError,
    anchor::WorldGenerationContext,
    material_condition::{
        BandRule, CompiledSurfaceRule, MaterialConditions, MaterialDensitySources, MaterialRule,
        MaterialRuleError, MaterialSurfaceQueries,
    },
    material_extensions::{ExtensionError, MaterialColumn, MaterialExtensions},
    terrain::{ColumnBounds, TerrainBlockStates, TerrainChunk, TerrainError},
};
use crate::{
    heightmap::{HeightmapState, HeightmapType},
    registry::BlockStateId,
};
use horizon_core::{
    density::{DensitySampler, SamplerContext},
    noise::DensityVolume,
    position::BlockPos,
    random::RandomSource,
};
use std::sync::Arc;

pub struct SurfaceInputs<'a, R: RandomSource, S: MaterialSurfaceQueries> {
    pub generation: WorldGenerationContext,
    pub noise_volume: DensityVolume,
    pub root: R::Positional,
    pub surface: S,
    pub preliminary_surface: Arc<dyn DensitySampler>,
    pub density_context: &'a mut SamplerContext,
    /// Inclusive generation write bounds, contained within the chunk.
    pub write_min_y: i32,
    pub write_max_y: i32,
}
pub struct TopMaterialInputs<'a, R: RandomSource, S: MaterialSurfaceQueries> {
    pub generation: WorldGenerationContext,
    pub root: R::Positional,
    pub surface: S,
    pub preliminary_surface: Arc<dyn DensitySampler>,
    pub density_context: &'a mut SamplerContext,
}
#[derive(Debug)]
pub enum SurfaceError {
    Terrain(TerrainError),
    Rule(MaterialRuleError),
    Decode(DecodeError),
    InvalidBounds,
    InvalidVolume,
    IterationLimit,
}
impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SurfaceError {}
impl From<TerrainError> for SurfaceError {
    fn from(e: TerrainError) -> Self {
        Self::Terrain(e)
    }
}
impl From<MaterialRuleError> for SurfaceError {
    fn from(e: MaterialRuleError) -> Self {
        Self::Rule(e)
    }
}
impl From<DecodeError> for SurfaceError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}
impl From<ExtensionError<SurfaceError>> for SurfaceError {
    fn from(e: ExtensionError<SurfaceError>) -> Self {
        match e {
            ExtensionError::Column(e) => e,
            ExtensionError::IterationLimit => Self::IterationLimit,
        }
    }
}

/// What the surface walk tells blocks apart by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockKind {
    Air,
    Fluid,
    Solid,
}
impl BlockKind {
    fn of<B: TerrainBlockStates>(chunk: &TerrainChunk<'_, B>, state: BlockStateId) -> Self {
        Self::from_properties(chunk.block_properties(state))
    }
    fn from_properties(properties: impl HeightmapState) -> Self {
        if properties.is_air() {
            Self::Air
        } else if properties.has_fluid() {
            Self::Fluid
        } else {
            Self::Solid
        }
    }
}
/// One column's resolved blocks and their kinds, refilled per column and
/// kept across columns.
#[derive(Default)]
struct ColumnCache {
    states: Vec<BlockStateId>,
    kinds: Vec<BlockKind>,
}
struct Column<'a, 'r, B: TerrainBlockStates> {
    chunk: &'a mut TerrainChunk<'r, B>,
    x: i32,
    z: i32,
    min: i32,
    max: i32,
    /// Blocks `low..=high`, updated by `set_block`. The surface walk reads a
    /// column twice (top to bottom, then the stone-depth look-behind); the
    /// palette decode and property lookup are paid once per block.
    cache: &'a mut ColumnCache,
    low: i32,
    high: i32,
    /// This column's `TerrainChunk::worldgen_column_bounds`: most writes are
    /// far below the heightmaps and need no update.
    bounds: Option<ColumnBounds>,
}
impl<'a, 'r, B: TerrainBlockStates> Column<'a, 'r, B> {
    fn new(
        chunk: &'a mut TerrainChunk<'r, B>,
        x: i32,
        z: i32,
        min: i32,
        max: i32,
        top: i32,
        cache: &'a mut ColumnCache,
    ) -> Self {
        let chunk_min = chunk.min_y();
        let chunk_max = chunk_min.wrapping_add(chunk.height()).wrapping_sub(1);
        let high = top.min(chunk_max);
        let low = min.max(chunk_min);
        cache.states.clear();
        cache.kinds.clear();
        if high >= low {
            let count = (high - low + 1) as usize;
            cache.states.reserve(count);
            cache.kinds.reserve(count);
            // An all-air or one-state section part comes as one run, and runs
            // of one state share a kind lookup.
            let blocks: &TerrainChunk<'r, B> = chunk;
            let mut last = None;
            blocks.column_runs(x, z, low, high, |state, run| {
                let kind = match last {
                    Some((seen, kind)) if seen == state => kind,
                    _ => {
                        let kind = BlockKind::of(blocks, state);
                        last = Some((state, kind));
                        kind
                    }
                };
                cache.states.extend(std::iter::repeat_n(state, run));
                cache.kinds.extend(std::iter::repeat_n(kind, run));
            });
        }
        let bounds = chunk.worldgen_column_bounds(x, z);
        Self {
            chunk,
            x,
            z,
            min,
            max,
            cache,
            low,
            high,
            bounds,
        }
    }
    fn cache_index(&self, y: i32) -> Option<usize> {
        (y >= self.low && y <= self.high).then(|| (y - self.low) as usize)
    }
    fn kind(&self, y: i32) -> BlockKind {
        match self.cache_index(y) {
            Some(index) => self.cache.kinds[index],
            None => BlockKind::of(self.chunk, self.chunk.block(self.x, y, self.z)),
        }
    }
    /// Whether `state` may be written at a whole run at once (`fill_opaque`):
    /// it is opaque for every heightmap the current stage keeps, and carries
    /// no fluid, so `set_block` would have marked no post-processing block.
    /// The column's bounds must be known: a missing map takes the single-block
    /// path, which primes it.
    fn fills_runs(&self, state: BlockStateId) -> bool {
        self.bounds.is_some() && self.chunk.stage_heightmaps_opaque(state)
    }
    /// Write `state` at every y in `low..=high`, high first, all of them solid
    /// blocks of this column: the top block takes the single-block path, the
    /// run below it the section fill.
    ///
    /// `state` is opaque, so `Heightmap::update` can only raise a map, at the
    /// y written: the top write raises it once and every lower y is then at or
    /// below `first_available - 2`, where the single-block path skips it. One
    /// update at the top therefore leaves the maps (and their bounds, which
    /// only follow a change) as a write per block leaves them.
    fn fill_opaque(
        &mut self,
        low: i32,
        high: i32,
        state: BlockStateId,
    ) -> Result<(), SurfaceError> {
        let high = high.min(self.max);
        let low = low.max(self.min);
        if high < low {
            return Ok(());
        }
        self.set_block(high, state)?;
        if high > low {
            match self.bounds {
                // A missing map takes the single-block path, which primes it.
                None => {
                    for y in (low..high).rev() {
                        self.set_block(y, state)?;
                    }
                    return Ok(());
                }
                Some(_) => {
                    self.chunk
                        .fill_solid_run(self.x, high - 1, self.z, high - low, state)?;
                }
            }
        }
        if let (Some(lo), Some(hi)) = (self.cache_index(low), self.cache_index(high)) {
            let kind = BlockKind::of(self.chunk, state);
            self.cache.states[lo..=hi].fill(state);
            self.cache.kinds[lo..=hi].fill(kind);
        }
        Ok(())
    }
}
impl<B: TerrainBlockStates> MaterialColumn for Column<'_, '_, B> {
    type Error = SurfaceError;
    fn block(&mut self, y: i32) -> Result<BlockStateId, SurfaceError> {
        Ok(match self.cache_index(y) {
            Some(index) => self.cache.states[index],
            None => self.chunk.block(self.x, y, self.z),
        })
    }
    fn set_block(&mut self, y: i32, state: BlockStateId) -> Result<(), SurfaceError> {
        if y >= self.min && y <= self.max {
            let properties = self.chunk.block_properties(state);
            // Most of the Nether's rule writes netherrack over netherrack.
            if self.block(y)? != state {
                self.chunk
                    .set_block_worldgen_column(self.x, y, self.z, state, &mut self.bounds)?;
                if let Some(index) = self.cache_index(y) {
                    self.cache.states[index] = state;
                    self.cache.kinds[index] = BlockKind::from_properties(properties);
                }
            }
            if properties.has_fluid() {
                self.chunk.mark_postprocessing(self.x, y, self.z);
            }
        }
        Ok(())
    }
    fn is_air(&self, state: BlockStateId) -> bool {
        self.chunk.block_properties(state).is_air()
    }
    fn same_block(&self, left: BlockStateId, right: BlockStateId) -> bool {
        self.chunk.same_block(left, right)
    }
}
fn first_available<B: TerrainBlockStates>(
    chunk: &mut TerrainChunk<'_, B>,
    x: i32,
    z: i32,
) -> Result<i32, SurfaceError> {
    if chunk.heightmap(HeightmapType::WorldSurfaceWg).is_none() {
        chunk.prime_heightmaps(&[HeightmapType::WorldSurfaceWg])?;
    }
    chunk
        .heightmap(HeightmapType::WorldSurfaceWg)
        .unwrap()
        .first_available(x, z)
        .map_err(TerrainError::from)
        .map_err(SurfaceError::from)
}
fn tick(remaining: &mut usize) -> Result<(), SurfaceError> {
    if *remaining == 0 {
        return Err(SurfaceError::IterationLimit);
    }
    *remaining -= 1;
    Ok(())
}

/// Apply a compiled top-material rule at one position with a worker the caller
/// built once. Carvers restore many cells per pass, so the rule is compiled and
/// the density bindings created once instead of per restored block.
pub(crate) fn top_material_cell<
    R: RandomSource,
    S: MaterialSurfaceQueries,
    B: TerrainBlockStates,
>(
    worker: &mut MaterialConditions<R, S>,
    chunk: &mut TerrainChunk<'_, B>,
    rule: usize,
    position: BlockPos,
    submerged: bool,
) -> Result<Option<BlockStateId>, SurfaceError> {
    let (lx, lz) = (position.x & 15, position.z & 15);
    let gx = first_available(chunk, (lx + 1).min(15), lz)?.wrapping_sub(first_available(
        chunk,
        (lx - 1).max(0),
        lz,
    )?);
    let gz = first_available(chunk, lx, (lz + 1).min(15))?.wrapping_sub(first_available(
        chunk,
        lx,
        (lz - 1).max(0),
    )?);
    worker.update_xz(position.x, position.z, gx, gz);
    worker.update_y(
        1,
        1,
        if submerged {
            position.y.wrapping_add(1)
        } else {
            i32::MIN
        },
        position.y,
    );
    worker.try_apply(rule, position).map_err(SurfaceError::from)
}

/// Evaluate the replacement surface under a carved block. Each call binds a
/// fresh one-cell material context, without restricting possible biomes, and
/// uses one stone block above/below. The caller applies the returned state.
/// Required heightmaps may be primed; block values are not changed here.
/// The caller's density context is retained on successful and failed evaluation.
pub fn top_material<R: RandomSource, S: MaterialSurfaceQueries, B: TerrainBlockStates>(
    chunk: &mut TerrainChunk<'_, B>,
    rule: MaterialRule,
    inputs: TopMaterialInputs<'_, R, S>,
    position: BlockPos,
    submerged: bool,
) -> Result<Option<BlockStateId>, SurfaceError> {
    let volume = DensityVolume::new([1; 3], [position.x, position.y, position.z], [1; 3])
        .map_err(|_| SurfaceError::InvalidVolume)?;
    let mut sources = MaterialDensitySources::new(
        inputs.surface,
        volume,
        inputs.preliminary_surface,
        SamplerContext::default(),
    )?;
    std::mem::swap(sources.sampler_context_mut(), inputs.density_context);
    let mut worker = MaterialConditions::<R, _>::with_possible_biomes(
        inputs.generation,
        inputs.root,
        sources,
        None,
    );
    let result = (|| {
        let rule = worker.add_rule(rule)?;
        top_material_cell(&mut worker, chunk, rule, position, submerged)
    })();
    let (_, mut context) = worker.into_surface().into_parts();
    std::mem::swap(&mut context, inputs.density_context);
    result
}

/// Build one chunk's surface in vanilla X/Z/descending-Y order. Required heightmaps
/// are updated during writes, so later columns observe earlier modifications.
/// Providers, rule roots, extensions and random settings must belong to one world.
/// Failures retain prior chunk writes and fluid postprocessing entries.
pub fn build_surface<R: RandomSource, S: MaterialSurfaceQueries, B: TerrainBlockStates>(
    chunk: &mut TerrainChunk<'_, B>,
    rule: MaterialRule,
    inputs: SurfaceInputs<'_, R, S>,
    extensions: &MaterialExtensions<R::Positional>,
) -> Result<(), SurfaceError>
where
    R::Positional: Clone,
{
    build_surface_shared(chunk, &rule, &mut None, inputs, extensions)
}
/// `build_surface` with the rule compiled once for the chunks `shared` serves
/// (`MaterialConditions::add_shared_rule`): one world's, one worker's.
pub fn build_surface_shared<R: RandomSource, S: MaterialSurfaceQueries, B: TerrainBlockStates>(
    chunk: &mut TerrainChunk<'_, B>,
    rule: &MaterialRule,
    shared: &mut Option<CompiledSurfaceRule<R>>,
    inputs: SurfaceInputs<'_, R, S>,
    extensions: &MaterialExtensions<R::Positional>,
) -> Result<(), SurfaceError>
where
    R::Positional: Clone,
{
    let min = chunk.min_y();
    let max = min + chunk.height() - 1;
    if inputs.write_min_y < min
        || inputs.write_max_y > max
        || inputs.write_min_y > inputs.write_max_y
    {
        return Err(SurfaceError::InvalidBounds);
    }
    let highest = chunk
        .highest_filled_section_index()
        .map_or(min - 1, |i| min + (i as i32) * 16 + 15);
    // The ore veins' densities are sampled over this volume, each over its
    // own vein's rows of it when one of them is first read
    // (`bind_density_rows`). Above the highest y a vein can place a block at,
    // no density is asked for. Cutting the volume's top keeps every value
    // below it (interpolation steps up from each cell's bottom).
    let top = highest.min(rule.ore_vein_top());
    let sizes = inputs.noise_volume.sizes();
    let origin = inputs.noise_volume.minimum();
    let height = top.wrapping_sub(origin[1]).wrapping_add(1).max(1);
    let expected = DensityVolume::new([sizes[0] as i32, height, sizes[2] as i32], origin, [1; 3])
        .map_err(|_| SurfaceError::InvalidVolume)?;
    let mut sources = MaterialDensitySources::new(
        inputs.surface,
        expected,
        inputs.preliminary_surface,
        SamplerContext::default(),
    )?;
    std::mem::swap(sources.sampler_context_mut(), inputs.density_context);
    let mut worker = MaterialConditions::<R, _>::new(inputs.generation, inputs.root, sources);
    let result = build_columns(
        chunk,
        rule,
        shared,
        &mut worker,
        extensions,
        inputs.write_min_y,
        inputs.write_max_y,
    );
    let (_, mut context) = worker.into_surface().into_parts();
    std::mem::swap(&mut context, inputs.density_context);
    result
}
fn build_columns<R: RandomSource, S: MaterialSurfaceQueries, B: TerrainBlockStates>(
    chunk: &mut TerrainChunk<'_, B>,
    rule: &MaterialRule,
    shared: &mut Option<CompiledSurfaceRule<R>>,
    worker: &mut MaterialConditions<R, S>,
    extensions: &MaterialExtensions<R::Positional>,
    write_min_y: i32,
    write_max_y: i32,
) -> Result<(), SurfaceError>
where
    R::Positional: Clone,
{
    let min = chunk.min_y();
    let rule = worker.add_shared_rule(rule, shared)?;
    let mut variants = worker.surface_variants(rule)?;
    let cx = chunk.position().x.wrapping_shl(4);
    let cz = chunk.position().z.wrapping_shl(4);
    // One block buffer, refilled per column and kept across columns.
    let mut cache = ColumnCache::default();
    for lx in 0..16 {
        for lz in 0..16 {
            let x = cx.wrapping_add(lx);
            let z = cz.wrapping_add(lz);
            let original_top = first_available(chunk, lx, lz)?;
            let biome = worker
                .biome_at(BlockPos::new(x, original_top, z))?
                .ok_or(MaterialRuleError::MissingBiome)?;
            if biome.as_ref() == "minecraft:eroded_badlands" {
                extensions.eroded_badlands(
                    &mut Column::new(
                        chunk,
                        x,
                        z,
                        write_min_y,
                        write_max_y,
                        original_top,
                        &mut cache,
                    ),
                    x,
                    z,
                    original_top,
                    min,
                )?;
            }
            let top = first_available(chunk, lx, lz)?;
            let gx = first_available(chunk, (lx + 1).min(15), lz)?.wrapping_sub(first_available(
                chunk,
                (lx - 1).max(0),
                lz,
            )?);
            let gz = first_available(chunk, lx, (lz + 1).min(15))?.wrapping_sub(first_available(
                chunk,
                lx,
                (lz - 1).max(0),
            )?);
            worker.update_xz(x, z, gx, gz);
            let mut above = 0i32;
            let mut water = i32::MIN;
            let mut bottom = i32::MAX;
            let mut y = top;
            let mut budget = 1 << 20;
            // The walk descends, so the band only steps down.
            let mut band = variants.band(top);
            let mut band_low = variants.band_bounds(band)[0];
            // The column's minimum surface level, read once when first needed.
            let mut minimum = None;
            let mut column = Column::new(chunk, x, z, write_min_y, write_max_y, top, &mut cache);
            while y >= min {
                tick(&mut budget)?;
                match column.kind(y) {
                    BlockKind::Air => {
                        above = 0;
                        water = i32::MIN;
                    }
                    BlockKind::Fluid => {
                        if water == i32::MIN {
                            water = y.wrapping_add(1);
                        }
                    }
                    BlockKind::Solid => {
                        if bottom >= y {
                            bottom = -32512; // DimensionType.MIN_Y << 4 for 26.3.
                            let mut below = y.wrapping_sub(1);
                            while below >= min - 1 {
                                tick(&mut budget)?;
                                if column.kind(below) != BlockKind::Solid {
                                    bottom = below.wrapping_add(1);
                                    break;
                                }
                                below = below.wrapping_sub(1);
                            }
                        }
                        above = above.wrapping_add(1);
                        if y >= write_min_y && y <= write_max_y {
                            let surface = variants.surface
                                && y >= match minimum {
                                    Some(level) => level,
                                    None => *minimum.insert(worker.minimum_surface_level()?),
                                };
                            while y < band_low {
                                band -= 1;
                                band_low = variants.band_bounds(band)[0];
                            }
                            // Where the rule gives no block, the block's context
                            // is not needed: each application sets its own first.
                            if let Some(band_rule) =
                                worker.surface_variant(&mut variants, band, surface)
                            {
                                worker.update_y(
                                    above,
                                    y.wrapping_sub(bottom).wrapping_add(1),
                                    water,
                                    y,
                                );
                                // A run of blocks the rule gives one state where
                                // none of its ore veins places one is written
                                // through the section fill, down to the first y
                                // where a vein can place a block, the band's
                                // bottom or the write bounds.
                                if let BandRule::Fill(_, index) = band_rule {
                                    let fill = variants.fill(index);
                                    if column.fills_runs(fill.block)
                                        && worker.veins_miss(fill, BlockPos::new(x, y, z))?
                                    {
                                        // A run above the minimum surface level stops
                                        // there too: below it the band takes its other
                                        // variant (`surface` is false).
                                        let lowest = match minimum {
                                            Some(level) if surface => {
                                                band_low.max(write_min_y).max(level)
                                            }
                                            _ => band_low.max(write_min_y),
                                        };
                                        let mut run_low = y;
                                        loop {
                                            let next = run_low.wrapping_sub(1);
                                            if next < lowest {
                                                break;
                                            }
                                            tick(&mut budget)?;
                                            if column.kind(next) != BlockKind::Solid
                                                || !worker
                                                    .veins_miss(fill, BlockPos::new(x, next, z))?
                                            {
                                                break;
                                            }
                                            run_low = next;
                                        }
                                        column.fill_opaque(run_low, y, fill.block)?;
                                        above = above.wrapping_add(y.wrapping_sub(run_low));
                                        // The run's blocks are written: the
                                        // loop continues below it.
                                        y = run_low.wrapping_sub(1);
                                        continue;
                                    }
                                }
                                if let Some(state) =
                                    worker.try_apply(band_rule.rule(), BlockPos::new(x, y, z))?
                                {
                                    column.set_block(y, state)?;
                                }
                            }
                        }
                    }
                }
                y = y.wrapping_sub(1);
            }
            if matches!(
                biome.as_ref(),
                "minecraft:frozen_ocean" | "minecraft:deep_frozen_ocean"
            ) {
                let minimum = worker.minimum_surface_level()?;
                extensions.frozen_ocean_with_melt(
                    &mut column,
                    [x, z],
                    original_top,
                    minimum,
                    |position, sea| {
                        worker
                            .melts_frozen_ocean(&biome, position, sea)
                            .map_err(SurfaceError::from)
                    },
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DATA_VERSION,
        heightmap::{HeightmapState, HeightmapType},
        nbt::Tag,
        registry::{BlockRegistry, BlockStateId},
        worldgen::terrain::{PreLightChunkStage, TerrainBlockStates},
    };
    use horizon_core::position::ChunkPos;

    const STONE: BlockStateId = BlockStateId(2);

    #[derive(Clone, Copy)]
    struct State(u32);
    impl HeightmapState for State {
        fn is_air(self) -> bool {
            self.0 < 2
        }
        fn is_air_block(self) -> bool {
            self.0 == 0
        }
        fn has_fluid(self) -> bool {
            self.0 == 3
        }
        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 == 2
        }
        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.0 == 2
        }
    }
    struct Properties;
    impl TerrainBlockStates for Properties {
        type State = State;
        fn heightmap_state(&self, id: BlockStateId) -> State {
            State(id.0)
        }
    }
    fn registry() -> BlockRegistry {
        let names = ["minecraft:air", "minecraft:void_air", "minecraft:stone"];
        let states = names.map(|name| Tag::compound([("id", Tag::String(name.into()))]));
        let defaults = names
            .iter()
            .enumerate()
            .map(|(id, name)| ((*name).into(), Tag::Int(id as i32)))
            .collect();
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("states", Tag::List(states.to_vec())),
            ("defaults", Tag::Compound(defaults)),
        ]))
        .unwrap()
    }
    /// A chunk whose columns are stone up to `top` and air above it, written
    /// without a stage, so neither heightmap exists yet.
    fn chunk(registry: &BlockRegistry, top: i32) -> TerrainChunk<'_, Properties> {
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(0, 0), -64, 128, registry, Properties).unwrap();
        chunk.set_generation_stage(PreLightChunkStage::Biomes);
        for y in -64..=top {
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set_block_raw(x, y, z, STONE).unwrap();
                }
            }
        }
        chunk
    }
    /// The path `fill_opaque` replaces: one `set_block` per y, top down.
    fn write_per_block(
        chunk: &mut TerrainChunk<'_, Properties>,
        x: i32,
        z: i32,
        low: i32,
        high: i32,
        state: BlockStateId,
    ) {
        let mut bounds = chunk.worldgen_column_bounds(x, z);
        for y in (low..=high).rev() {
            if chunk.block(x, y, z) != state {
                chunk
                    .set_block_worldgen_column(x, y, z, state, &mut bounds)
                    .unwrap();
            }
        }
    }
    fn assert_same(a: &TerrainChunk<'_, Properties>, b: &TerrainChunk<'_, Properties>) {
        for section in 0..a.height() as usize / 16 {
            assert_eq!(
                a.section(section).unwrap().to_nbt().unwrap(),
                b.section(section).unwrap().to_nbt().unwrap(),
                "section {section}"
            );
        }
        for kind in HeightmapType::ALL {
            assert_eq!(
                a.heightmap(kind).map(|map| map.raw_data()),
                b.heightmap(kind).map(|map| map.raw_data()),
                "{kind:?}"
            );
        }
        assert_eq!(a.postprocessing(), b.postprocessing());
        for y in a.min_y()..a.min_y() + a.height() {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(a.block(x, y, z), b.block(x, y, z), "{x} {y} {z}");
                }
            }
        }
    }

    /// A run fill leaves the chunk and its heightmaps exactly as one write per
    /// block does, with the maps missing (so the first write primes them) and
    /// present, over runs that start in air, in stone or on the chunk's floor.
    #[test]
    fn fill_opaque_writes_what_a_write_per_block_writes() {
        let registry = registry();
        for primed in [false, true] {
            for (top, low, high) in [
                (0, -40, 12),
                (0, -64, 63),
                (0, -16, -1),
                (0, 0, 0),
                (0, 5, 12),
                (40, -60, 40),
            ] {
                let (mut per_block, mut filled) = (chunk(&registry, top), chunk(&registry, top));
                if primed {
                    for chunk in [&mut per_block, &mut filled] {
                        chunk.prime_heightmaps(&HeightmapType::ALL).unwrap();
                    }
                }
                write_per_block(&mut per_block, 3, 5, low, high, STONE);
                let mut cache = ColumnCache::default();
                Column::new(&mut filled, 3, 5, -64, 63, 63, &mut cache)
                    .fill_opaque(low, high, STONE)
                    .unwrap();
                assert_same(&per_block, &filled);
            }
        }
        // A write that changes nothing either way, and one the write bounds
        // clip, leave the same chunk.
        let (mut per_block, mut filled) = (chunk(&registry, 0), chunk(&registry, 0));
        write_per_block(&mut per_block, 0, 0, -70, -64, STONE);
        let mut cache = ColumnCache::default();
        Column::new(&mut filled, 0, 0, -64, 0, 0, &mut cache)
            .fill_opaque(-70, -64, STONE)
            .unwrap();
        assert_same(&per_block, &filled);
        // Below the chunk: the run stops at the write bounds.
        assert_ne!(filled.block(0, -70, 0), STONE);
    }
}
