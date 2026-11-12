//! Preliminary surface density sampling scoped to one material context.
use super::{MaterialRuleError, MaterialSurfaceQueries};
use crate::{registry::BlockStateId, worldgen::DecodeError};
use horizon_core::{
    density::{DensitySampler, SamplerContext},
    noise::{DensityVolume, Noise},
    position::BlockPos,
};
use std::{collections::BTreeSet, sync::Arc};

/// A lazy unit-spaced XZ plane and scalar fallback for preliminary heights.
/// Construct a fresh provider for each material context, with that world's
/// compiled preliminary-surface density root and bound sampler context.
pub struct MaterialDensitySources<S: MaterialSurfaceQueries> {
    surface: S,
    expected: DensityVolume,
    densities: Vec<BoundDensity>,
    /// Sampled volumes, shared by bindings of the same density root over the
    /// same volume.
    buffers: Vec<DensityBuffer>,
    retained_samples: usize,
    plane: DensityVolume,
    sampler: Arc<dyn DensitySampler>,
    context: SamplerContext,
    buffer: Option<Vec<f32>>,
}
struct BoundDensity {
    sampler: Arc<dyn DensitySampler>,
    buffer: Option<usize>,
}
/// One density root's values over `volume`. A vein's rows of a chunk volume
/// (`bind_density_rows`), when they are less than the whole of it, are
/// sampled when first read and `values` is empty until then; every other
/// volume is sampled when bound.
struct DensityBuffer {
    values: Vec<f32>,
    volume: DensityVolume,
    /// `volume`'s minimum and sizes for the read of a block, which happens
    /// once or twice per block of a column; the sizes are 0 for a volume
    /// whose steps are not 1, which `density_outside` reads.
    minimum: [i32; 3],
    sizes: [usize; 3],
}
impl DensityBuffer {
    fn new(values: Vec<f32>, volume: DensityVolume) -> Self {
        Self {
            values,
            volume,
            minimum: volume.minimum(),
            sizes: if volume.steps() == [1; 3] {
                volume.sizes()
            } else {
                [0; 3]
            },
        }
    }
}
/// The rows of `expected`, a volume of unit steps, that a density read only
/// at a y in `rows` is sampled over; `None` when it has none of them. They
/// are widened to whole 16-block rows of the world: interpolation cells are
/// anchored on absolute coordinates and divide 16, so the volume keeps whole
/// cells, each with the corners and the values it has in `expected`.
fn rows_volume(expected: DensityVolume, rows: [i32; 2]) -> Option<DensityVolume> {
    let [sx, sy, sz] = expected.sizes();
    let [x, bottom, z] = expected.minimum();
    let bottom = i64::from(bottom);
    let top = bottom + sy as i64 - 1;
    let (low, high) = (i64::from(rows[0]).max(bottom), i64::from(rows[1]).min(top));
    if low > high {
        return None;
    }
    let low = (low.div_euclid(16) * 16).max(bottom);
    let high = (high.div_euclid(16) * 16 + 15).min(top);
    DensityVolume::new(
        [sx as i32, (high - low + 1) as i32, sz as i32],
        [x, low as i32, z],
        [1; 3],
    )
    .ok()
}
impl<S: MaterialSurfaceQueries> MaterialDensitySources<S> {
    pub fn sampler_context_mut(&mut self) -> &mut SamplerContext {
        &mut self.context
    }
    /// Re-target the per-cell bindings to one block. A carver reuses one worker
    /// across every restored dirt block, so the 1×1×1 volumes the bindings and
    /// the preliminary plane sample must follow the cell; the compiled rule is
    /// kept.
    pub fn retarget(&mut self, position: [i32; 3]) -> Result<(), DecodeError> {
        let volume = DensityVolume::new([1; 3], position, [1; 3])
            .map_err(|_| DecodeError::Field("material density volume"))?;
        let plane = DensityVolume::new([1; 3], [position[0], 0, position[2]], [1; 3])
            .map_err(|_| DecodeError::Field("material preliminary plane"))?;
        self.expected = volume;
        self.plane = plane;
        // The plane is sampled lazily at the new x/z.
        self.buffer = None;
        let mut sampled = vec![false; self.buffers.len()];
        let mut pending = Vec::new();
        for bound in &self.densities {
            let Some(buffer) = bound.buffer else { continue };
            if sampled.get(buffer).copied().unwrap_or(true) {
                continue;
            }
            sampled[buffer] = true;
            pending.push((bound.sampler.clone(), buffer));
        }
        for (sampler, buffer) in pending {
            let mut values = std::mem::take(&mut self.buffers[buffer].values);
            values.resize(volume.size(), 0.0);
            let sampled = sampler.sample_volume(&mut self.context, &mut values, volume);
            self.buffers[buffer] = DensityBuffer::new(values, volume);
            sampled?;
        }
        Ok(())
    }
    pub fn into_parts(self) -> (S, SamplerContext) {
        (self.surface, self.context)
    }
    pub fn new(
        surface: S,
        expected: DensityVolume,
        sampler: Arc<dyn DensitySampler>,
        context: SamplerContext,
    ) -> Result<Self, DecodeError> {
        let [sx, _, sz] = expected.sizes();
        // Bound retained allocation. Normal chunk planes contain 256 samples.
        if sx * sz > 1 << 20 {
            return Err(DecodeError::Field("material preliminary plane size"));
        }
        let [x, _, z] = expected.minimum();
        let plane = DensityVolume::new([sx as i32, 1, sz as i32], [x, 0, z], [1; 3])
            .map_err(|_| DecodeError::Field("material preliminary plane"))?;
        Ok(Self {
            surface,
            expected,
            densities: Vec::new(),
            buffers: Vec::new(),
            retained_samples: 0,
            plane,
            sampler,
            context,
            buffer: None,
        })
    }
}
impl<S: MaterialSurfaceQueries> MaterialDensitySources<S> {
    fn bind(
        &mut self,
        sampler: Arc<dyn DensitySampler>,
        eager: bool,
        rows: Option<[i32; 2]>,
    ) -> Result<usize, DecodeError> {
        if self.densities.len() >= 32768
            || (eager
                && (self.expected.size() > 1 << 20
                    || self.retained_samples + self.expected.size() > 1 << 24))
        {
            return Err(DecodeError::Field("material density storage bound"));
        }
        // A one-block volume (a carver's restored cell) and a volume whose
        // steps are not 1 (its rows are not blocks) are sampled whole, now.
        let narrowed = rows
            .filter(|_| self.expected.sizes()[1] > 1 && self.expected.steps() == [1; 3])
            .map(|rows| rows_volume(self.expected, rows));
        let buffer = match (eager, narrowed) {
            // No row of the volume is read: as a lazy binding, nothing is kept.
            (false, _) | (true, Some(None)) => None,
            (true, narrowed) => {
                // Rows that leave the volume whole (a vein of unknown bounds)
                // are sampled now, as a plain binding is.
                let (volume, sample) = match narrowed {
                    Some(Some(volume)) if volume != self.expected => (volume, false),
                    _ => (self.expected, true),
                };
                // Sampling a root over the same volume and context is
                // deterministic, so a second binding reuses the first's.
                let shared = self
                    .densities
                    .iter()
                    .filter(|bound| Arc::ptr_eq(&bound.sampler, &sampler))
                    .filter_map(|bound| bound.buffer)
                    .find(|&buffer| self.buffers[buffer].volume == volume);
                Some(match shared {
                    Some(buffer) => buffer,
                    None => {
                        let mut values = Vec::new();
                        if sample {
                            values.resize(volume.size(), 0.0);
                            sampler.sample_volume(&mut self.context, &mut values, volume)?;
                        }
                        self.retained_samples += volume.size();
                        self.buffers.push(DensityBuffer::new(values, volume));
                        self.buffers.len() - 1
                    }
                })
            }
        };
        let index = self.densities.len();
        self.densities.push(BoundDensity { sampler, buffer });
        Ok(index)
    }
}
impl<S: MaterialSurfaceQueries> MaterialDensitySources<S> {
    /// `density` where the block is not among sampled unit-step values:
    /// rows at their first read are sampled now (a failed sample leaves them
    /// unsampled), and a position outside the binding's volume, or a binding
    /// without one, is sampled alone.
    #[cold]
    #[inline(never)]
    fn density_outside(
        &mut self,
        index: usize,
        position: BlockPos,
    ) -> Result<f32, MaterialRuleError> {
        let source = &self.densities[index];
        let position = [position.x, position.y, position.z];
        if let Some(buffer) = source.buffer {
            let out = &mut self.buffers[buffer];
            if let Some(at) = block_index(out.volume, position) {
                if out.values.is_empty() {
                    out.values.resize(out.volume.size(), 0.0);
                    let sampled = source.sampler.sample_volume(
                        &mut self.context,
                        &mut out.values,
                        out.volume,
                    );
                    if let Err(error) = sampled {
                        out.values.clear();
                        return Err(error.into());
                    }
                }
                return Ok(out.values[at]);
            }
        }
        Ok(source.sampler.sample_value(&mut self.context, position)?)
    }
}
impl<S: MaterialSurfaceQueries> MaterialSurfaceQueries for MaterialDensitySources<S> {
    fn bind_density(
        &mut self,
        sampler: Arc<dyn DensitySampler>,
        eager: bool,
    ) -> Result<usize, DecodeError> {
        self.bind(sampler, eager, None)
    }
    fn bind_density_rows(
        &mut self,
        sampler: Arc<dyn DensitySampler>,
        eager: bool,
        rows: [i32; 2],
    ) -> Result<usize, DecodeError> {
        self.bind(sampler, eager, Some(rows))
    }
    #[inline]
    fn density(&mut self, index: usize, position: BlockPos) -> Result<f32, MaterialRuleError> {
        let source = self
            .densities
            .get(index)
            .ok_or(MaterialRuleError::MissingDensityProvider)?;
        if let Some(buffer) = source.buffer {
            let rows = &self.buffers[buffer];
            let [sx, sy, sz] = rows.sizes;
            let x = position.x.wrapping_sub(rows.minimum[0]) as u32 as usize;
            let y = position.y.wrapping_sub(rows.minimum[1]) as u32 as usize;
            let z = position.z.wrapping_sub(rows.minimum[2]) as u32 as usize;
            if x < sx
                && y < sy
                && z < sz
                && let Some(value) = rows.values.get(y + (x + z * sx) * sy)
            {
                return Ok(*value);
            }
        }
        self.density_outside(index, position)
    }
    fn preliminary_surface(&mut self, position: BlockPos) -> Result<f32, MaterialRuleError> {
        let position = [position.x, 0, position.z];
        if let Some(index) = self.plane.index_of_block(position) {
            if self.buffer.is_none() {
                // Retain the allocation and partial writes if sampling fails.
                let buffer = self.buffer.insert(vec![0.0; self.plane.size()]);
                self.sampler
                    .sample_volume(&mut self.context, buffer, self.plane)?;
            }
            Ok(self.buffer.as_ref().unwrap()[index])
        } else {
            Ok(self.sampler.sample_value(&mut self.context, position)?)
        }
    }
    fn possible_biomes(&self) -> Option<BTreeSet<String>> {
        self.surface.possible_biomes()
    }
    fn biome(&mut self, position: BlockPos) -> Result<Option<Arc<str>>, MaterialRuleError> {
        self.surface.biome(position)
    }
    fn biome_id(&mut self, position: BlockPos) -> Result<Option<u32>, MaterialRuleError> {
        self.surface.biome_id(position)
    }
    fn biome_index(&self, name: &str) -> Option<Option<u32>> {
        self.surface.biome_index(name)
    }
    fn melts_frozen_ocean(
        &mut self,
        biome: &str,
        position: BlockPos,
        sea_level: i32,
    ) -> Result<bool, MaterialRuleError> {
        self.surface.melts_frozen_ocean(biome, position, sea_level)
    }
    fn cold_enough_to_snow(
        &mut self,
        biome: &str,
        position: BlockPos,
        sea_level: i32,
    ) -> Result<bool, MaterialRuleError> {
        self.surface.cold_enough_to_snow(biome, position, sea_level)
    }
    fn bind_noise(&mut self, name: &str) -> Result<Arc<dyn Noise>, DecodeError> {
        self.surface.bind_noise(name)
    }
    fn surface_depth(&mut self, x: i32, z: i32) -> i32 {
        self.surface.surface_depth(x, z)
    }
    fn surface_secondary(&mut self, x: i32, z: i32) -> f64 {
        self.surface.surface_secondary(x, z)
    }
    fn band(&mut self, position: BlockPos) -> Result<BlockStateId, MaterialRuleError> {
        self.surface.band(position)
    }
}

/// `DensityVolume::index_of_block` without its per-axis divisions for the
/// unit-step volumes the material stage samples (one or two lookups per block).
#[inline]
fn block_index(volume: DensityVolume, position: [i32; 3]) -> Option<usize> {
    if volume.steps() != [1; 3] {
        return volume.index_of_block(position);
    }
    let [mx, my, mz] = volume.minimum();
    let [sx, sy, sz] = volume.sizes();
    let x = position[0].wrapping_sub(mx) as u32 as usize;
    let y = position[1].wrapping_sub(my) as u32 as usize;
    let z = position[2].wrapping_sub(mz) as u32 as usize;
    (x < sx && y < sy && z < sz).then(|| y + (x + z * sx) * sy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::density::SampleError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Surface;
    impl MaterialSurfaceQueries for Surface {
        fn surface_depth(&mut self, _: i32, _: i32) -> i32 {
            0
        }
        fn surface_secondary(&mut self, _: i32, _: i32) -> f64 {
            0.0
        }
    }
    /// Position-dependent values; counts bulk samples.
    #[derive(Debug, Default)]
    struct Counted(AtomicUsize);
    impl DensitySampler for Counted {
        fn sample_value(&self, _: &mut SamplerContext, p: [i32; 3]) -> Result<f32, SampleError> {
            Ok((p[0] * 10_000 + p[1] * 100 + p[2]) as f32)
        }
        fn sample_volume(
            &self,
            context: &mut SamplerContext,
            buffer: &mut [f32],
            volume: DensityVolume,
        ) -> Result<(), SampleError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            for z in 0..volume.sizes()[2] {
                for x in 0..volume.sizes()[0] {
                    for y in 0..volume.sizes()[1] {
                        let [mx, my, mz] = volume.minimum();
                        let p = [mx + x as i32, my + y as i32, mz + z as i32];
                        buffer[volume.index([x, y, z]).unwrap()] = self.sample_value(context, p)?;
                    }
                }
            }
            Ok(())
        }
    }
    fn sources() -> MaterialDensitySources<Surface> {
        let volume = DensityVolume::new([16, 24, 16], [32, -64, -16], [1; 3]).unwrap();
        MaterialDensitySources::new(
            Surface,
            volume,
            Arc::new(Counted::default()),
            SamplerContext::default(),
        )
        .unwrap()
    }

    #[test]
    fn eager_bindings_of_one_root_share_a_single_sample() {
        let mut sources = sources();
        let shared = Arc::new(Counted::default());
        let other = Arc::new(Counted::default());
        let a = sources.bind_density(shared.clone(), true).unwrap();
        let lazy = sources.bind_density(shared.clone(), false).unwrap();
        let b = sources.bind_density(shared.clone(), true).unwrap();
        let c = sources.bind_density(other.clone(), true).unwrap();
        assert_eq!(shared.0.load(Ordering::Relaxed), 1);
        assert_eq!(other.0.load(Ordering::Relaxed), 1);
        assert_eq!([a, lazy, b, c], [0, 1, 2, 3]);
        for p in [[32, -64, -16], [47, -41, -1], [40, -50, -9], [48, 0, 0]] {
            let position = BlockPos::new(p[0], p[1], p[2]);
            let expected = (p[0] * 10_000 + p[1] * 100 + p[2]) as f32;
            for index in [a, lazy, b, c] {
                assert_eq!(sources.density(index, position).unwrap(), expected);
            }
        }
        assert_eq!(shared.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_veins_rows_are_sampled_at_their_first_read_and_only_they() {
        let chunk = DensityVolume::new([16, 115, 16], [32, -64, -16], [1; 3]).unwrap();
        let rows = |rows| rows_volume(chunk, rows).map(|v| (v.minimum()[1], v.sizes()[1]));
        assert_eq!(rows([0, 50]), Some((0, 51)));
        assert_eq!(rows([-60, -8]), Some((-64, 64)));
        assert_eq!(rows([i32::MIN, i32::MAX]), Some((-64, 115)));
        assert_eq!(rows([60, 70]), None);
        assert_eq!(rows([i32::MIN, -65]), None);
        let mut sources = MaterialDensitySources::new(
            Surface,
            chunk,
            Arc::new(Counted::default()),
            SamplerContext::default(),
        )
        .unwrap();
        let (copper, iron, never) = (
            Arc::new(Counted::default()),
            Arc::new(Counted::default()),
            Arc::new(Counted::default()),
        );
        let a = sources
            .bind_density_rows(copper.clone(), true, [0, 50])
            .unwrap();
        let b = sources
            .bind_density_rows(iron.clone(), true, [-60, -8])
            .unwrap();
        let c = sources
            .bind_density_rows(copper.clone(), true, [3, 50])
            .unwrap();
        let d = sources
            .bind_density_rows(never.clone(), true, [60, 70])
            .unwrap();
        let samples = |counted: &Counted| counted.0.load(Ordering::Relaxed);
        assert_eq!([samples(&copper), samples(&iron), samples(&never)], [0; 3]);
        // Rows that are the whole volume are sampled when bound.
        let whole = Arc::new(Counted::default());
        let e = sources
            .bind_density_rows(whole.clone(), true, [i32::MIN, i32::MAX])
            .unwrap();
        assert_eq!(samples(&whole), 1);
        assert_eq!(
            sources.density(e, BlockPos::new(33, 7, -2)).unwrap(),
            330_698.0
        );
        // `Counted` gives every position its own value, whichever way it is asked.
        for (index, y) in [
            (a, 0),
            (a, 50),
            (c, 17),
            (b, -60),
            (b, -8),
            (b, -64),
            (a, -3),
            (d, 2),
        ] {
            for (x, z) in [(32, -16), (47, -1), (40, -9)] {
                let expected = (x * 10_000 + y * 100 + z) as f32;
                let position = BlockPos::new(x, y, z);
                assert_eq!(sources.density(index, position).unwrap(), expected, "{y}");
            }
        }
        // One volume per root and rows; the rows outside the chunk have none.
        assert_eq!(
            [samples(&copper), samples(&iron), samples(&never)],
            [1, 1, 0]
        );
    }

    /// Records the volumes it is asked for; values as `Counted`'s.
    #[derive(Debug, Default)]
    struct Recorded(std::sync::Mutex<Vec<DensityVolume>>);
    impl DensitySampler for Recorded {
        fn sample_value(&self, _: &mut SamplerContext, p: [i32; 3]) -> Result<f32, SampleError> {
            Ok((p[0] * 10_000 + p[1] * 100 + p[2]) as f32)
        }
        fn sample_volume(
            &self,
            context: &mut SamplerContext,
            buffer: &mut [f32],
            volume: DensityVolume,
        ) -> Result<(), SampleError> {
            self.0.lock().unwrap().push(volume);
            let ([mx, my, mz], [dx, dy, dz]) = (volume.minimum(), volume.steps());
            for z in 0..volume.sizes()[2] {
                for x in 0..volume.sizes()[0] {
                    for y in 0..volume.sizes()[1] {
                        let p = [mx + x as i32 * dx, my + y as i32 * dy, mz + z as i32 * dz];
                        buffer[volume.index([x, y, z]).unwrap()] = self.sample_value(context, p)?;
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn rows_of_a_volume_of_other_steps_are_sampled_whole_when_bound() {
        // A 3 x 2 x 3 lattice of 4 x 8 x 4 cells: y -5 and 3.
        let lattice = DensityVolume::new([3, 2, 3], [-10, -5, -15], [4, 8, 4]).unwrap();
        for rows in [[i32::MIN, i32::MAX], [0, 50], [60, 70]] {
            let mut sources = MaterialDensitySources::new(
                Surface,
                lattice,
                Arc::new(Counted::default()),
                SamplerContext::default(),
            )
            .unwrap();
            let vein = Arc::new(Recorded::default());
            let index = sources.bind_density_rows(vein.clone(), true, rows).unwrap();
            assert_eq!(*vein.0.lock().unwrap(), [lattice], "{rows:?}");
            // On the lattice, between its points and outside it.
            for p in [
                [-10, -5, -15],
                [-2, 3, -7],
                [-6, 3, -11],
                [-9, -5, -15],
                [0, 0, 0],
            ] {
                let expected = (p[0] * 10_000 + p[1] * 100 + p[2]) as f32;
                let position = BlockPos::new(p[0], p[1], p[2]);
                assert_eq!(sources.density(index, position).unwrap(), expected, "{p:?}");
            }
            assert_eq!(vein.0.lock().unwrap().len(), 1, "{rows:?}");
        }
    }

    #[test]
    fn block_index_matches_index_of_block() {
        let unit = DensityVolume::new([16, 24, 16], [i32::MAX - 7, -64, -16], [1; 3]).unwrap();
        let strided = DensityVolume::new([5, 3, 5], [-8, -64, 0], [4, 8, 4]).unwrap();
        for volume in [unit, strided] {
            let [mx, my, mz] = volume.minimum();
            for dx in [-1, 0, 1, 4, 7, 8, 15, 16, 17, 20] {
                for dy in [-9, -1, 0, 8, 16, 23, 24] {
                    for dz in [-1, 0, 3, 15, 16, 20] {
                        let p = [
                            mx.wrapping_add(dx),
                            my.wrapping_add(dy),
                            mz.wrapping_add(dz),
                        ];
                        assert_eq!(block_index(volume, p), volume.index_of_block(p), "{p:?}");
                    }
                }
            }
            for p in [
                [i32::MIN, 0, 0],
                [i32::MAX, i32::MAX, i32::MAX],
                [0, i32::MIN, 0],
            ] {
                assert_eq!(block_index(volume, p), volume.index_of_block(p), "{p:?}");
            }
        }
    }
}
