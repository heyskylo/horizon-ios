//! Stateful 26.3 underground fluid and pressure selection.
use super::{AquiferField, AquiferSamplers, FluidPicker, FluidStatus};
use crate::registry::{BlockRegistry, BlockStateId};
use horizon_core::{
    density::{SampleError, SamplerContext},
    noise::{DensityError, DensityVolume},
    position::{BlockPos, ChunkPos},
    random::{PositionalRandom, RandomSource},
};
use std::collections::HashMap;

const WAY_BELOW_MIN_Y: i32 = -32512;
const FLOWING_UPDATE_SIMILARITY: f64 = 1.0 - 44.0 / 25.0;
const SURFACE_OFFSETS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// All IDs and block classifications must refer to the same immutable registry.
/// Lava/water tests classify the block, whereas default-lava equality tests the
/// exact state. Air tests include cave air and void air.
pub trait AquiferBlockStates {
    fn air(&self) -> BlockStateId;
    fn lava(&self) -> BlockStateId;
    fn is_air(&self, state: BlockStateId) -> bool;
    fn is_lava(&self, state: BlockStateId) -> bool;
    fn is_water(&self, state: BlockStateId) -> bool;
}
pub struct RegistryAquiferBlockStates<'a> {
    registry: &'a BlockRegistry,
    air: BlockStateId,
    lava: BlockStateId,
    /// The default water state, if the registry has water.
    water: Option<BlockStateId>,
}
impl<'a> RegistryAquiferBlockStates<'a> {
    pub fn new(registry: &'a BlockRegistry) -> Result<Self, AquiferError> {
        Ok(Self {
            registry,
            air: registry
                .default_state("minecraft:air")
                .ok_or(AquiferError::MissingBlock)?,
            lava: registry
                .default_state("minecraft:lava")
                .ok_or(AquiferError::MissingBlock)?,
            water: registry.default_state("minecraft:water"),
        })
    }
    fn name(&self, state: BlockStateId) -> Option<&str> {
        self.registry.state(state).map(|s| s.name.as_str())
    }
}
// Almost every state asked about is default air, water or lava, whose blocks
// are known without reading their names; any other state reads its name.
impl AquiferBlockStates for RegistryAquiferBlockStates<'_> {
    fn air(&self) -> BlockStateId {
        self.air
    }
    fn lava(&self) -> BlockStateId {
        self.lava
    }
    fn is_air(&self, state: BlockStateId) -> bool {
        state == self.air
            || (state != self.lava
                && Some(state) != self.water
                && matches!(
                    self.name(state),
                    Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
                ))
    }
    fn is_lava(&self, state: BlockStateId) -> bool {
        state == self.lava
            || (state != self.air
                && Some(state) != self.water
                && self.name(state) == Some("minecraft:lava"))
    }
    fn is_water(&self, state: BlockStateId) -> bool {
        Some(state) == self.water
            || (state != self.air
                && state != self.lava
                && self.name(state) == Some("minecraft:water"))
    }
}
#[derive(Debug)]
pub enum AquiferError {
    Sample(SampleError),
    InvalidGrid,
    OutsideGrid,
    Allocation,
    MissingBlock,
}
impl From<SampleError> for AquiferError {
    fn from(value: SampleError) -> Self {
        Self::Sample(value)
    }
}
impl From<DensityError> for AquiferError {
    fn from(value: DensityError) -> Self {
        Self::Sample(value.into())
    }
}
impl std::fmt::Display for AquiferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sample(e) => e.fmt(f),
            Self::InvalidGrid => f.write_str("aquifer grid is invalid or exceeds 16777216 cells"),
            Self::OutsideGrid => f.write_str("aquifer query lies outside its grid"),
            Self::Allocation => f.write_str("aquifer allocation failed"),
            Self::MissingBlock => f.write_str("aquifer registry lacks default air or lava"),
        }
    }
}
impl std::error::Error for AquiferError {}

/// The 12 grid cells around one anchor, in vanilla's `x, y, z` loop order.
#[derive(Clone, Copy)]
struct Cells {
    /// The cell's index in the grid's caches.
    index: [u32; 12],
    x: [i32; 12],
    y: [i32; 12],
    z: [i32; 12],
    /// `dx * dx + dz * dz` from the cell's location to the current column.
    horizontal: [i32; 12],
}

pub struct NoiseBasedAquifer<P, F, B> {
    samplers: AquiferSamplers,
    picker: P,
    random: F,
    blocks: B,
    minimum: [i32; 3],
    size_x: i32,
    size_z: i32,
    statuses: Vec<Option<FluidStatus>>,
    locations: Vec<i64>,
    /// Each drawn location as `BlockPos.getX/Y/Z` read it back.
    points: Vec<[i32; 3]>,
    surfaces: HashMap<i64, i32>,
    /// The anchor `cells` holds, once all 12 of its cells are in the grid.
    anchor: Option<[i32; 3]>,
    /// The column `cells.horizontal` was computed for.
    column: [i32; 2],
    cells: Cells,
    skip_above_y: i32,
    schedule: bool,
}
impl<P: FluidPicker, F: PositionalRandom, B: AquiferBlockStates> NoiseBasedAquifer<P, F, B> {
    /// The positional factory is the caller's aquifer factory, not the world's
    /// unmodified factory. Every sampler uses the caller's shared context.
    pub fn new(
        samplers: AquiferSamplers,
        picker: P,
        random: F,
        blocks: B,
        volume: DensityVolume,
        context: &mut SamplerContext,
    ) -> Result<Self, AquiferError> {
        let low = volume.minimum();
        let high = volume.maximum();
        let minimum = [
            low[0].wrapping_sub(5) >> 4,
            low[1].wrapping_add(1).div_euclid(12).wrapping_sub(1),
            low[2].wrapping_sub(5) >> 4,
        ];
        let maximum = [
            (high[0].wrapping_sub(5) >> 4).wrapping_add(1),
            high[1].wrapping_add(1).div_euclid(12).wrapping_add(1),
            (high[2].wrapping_sub(5) >> 4).wrapping_add(1),
        ];
        let sizes: [i32; 3] =
            std::array::from_fn(|i| maximum[i].wrapping_sub(minimum[i]).wrapping_add(1));
        let count = sizes
            .into_iter()
            .try_fold(1usize, |n, s| {
                if s > 0 {
                    n.checked_mul(s as usize)
                } else {
                    None
                }
            })
            .filter(|n| *n <= 16_777_216)
            .ok_or(AquiferError::InvalidGrid)?;
        let mut statuses = Vec::new();
        statuses
            .try_reserve_exact(count)
            .map_err(|_| AquiferError::Allocation)?;
        statuses.resize(count, None);
        let mut locations = Vec::new();
        locations
            .try_reserve_exact(count)
            .map_err(|_| AquiferError::Allocation)?;
        locations.resize(count, i64::MAX);
        let mut points = Vec::new();
        points
            .try_reserve_exact(count)
            .map_err(|_| AquiferError::Allocation)?;
        points.resize(count, [0; 3]);
        let mut result = Self {
            samplers,
            picker,
            random,
            blocks,
            minimum,
            size_x: sizes[0],
            size_z: sizes[2],
            statuses,
            locations,
            points,
            surfaces: HashMap::new(),
            anchor: None,
            column: [0; 2],
            cells: Cells {
                index: [0; 12],
                x: [0; 12],
                y: [0; 12],
                z: [0; 12],
                horizontal: [0; 12],
            },
            skip_above_y: 0,
            schedule: false,
        };
        let max_surface = result
            .max_surface_level(
                context,
                [minimum[0].wrapping_shl(4), minimum[2].wrapping_shl(4)],
                [
                    maximum[0].wrapping_shl(4).wrapping_add(9),
                    maximum[2].wrapping_shl(4).wrapping_add(9),
                ],
            )?
            .wrapping_add(8);
        let skip_grid = max_surface.wrapping_add(12).div_euclid(12).wrapping_add(1);
        result.skip_above_y = skip_grid.wrapping_mul(12).wrapping_add(11).wrapping_sub(1);
        Ok(result)
    }
    /// Read-only diagnostic state; enumeration never populates a cache.
    pub fn cached_cells(&self) -> impl Iterator<Item = (i64, Option<FluidStatus>)> + '_ {
        self.locations
            .iter()
            .copied()
            .zip(self.statuses.iter().copied())
    }
    pub fn cached_surfaces(&self) -> impl Iterator<Item = (i64, i32)> + '_ {
        self.surfaces.iter().map(|(key, value)| (*key, *value))
    }
    pub fn should_schedule_fluid_update(&self) -> bool {
        self.schedule
    }
    pub fn skip_sampling_above_y(&self) -> i32 {
        self.skip_above_y
    }
    /// The fill decided a block without this aquifer. Both old decisions
    /// (positive density and air above the skip height) cleared the pending
    /// fluid update, so the caller reproduces that side effect.
    pub fn note_skipped(&mut self) {
        self.schedule = false;
    }
    /// The y above which a non-positive density is the global fluid's air. The
    /// vanilla fluid rules depend only on y; a picker that varies with x/z keeps
    /// the per-block call by answering `None`.
    pub fn air_above_y(&self) -> Option<i32> {
        let center = self.picker.compute([0, i32::MAX, 0]);
        let east = self.picker.compute([i32::MAX, i32::MAX, i32::MAX]);
        let west = self.picker.compute([i32::MIN, i32::MAX, i32::MIN]);
        (center.level == east.level && center.level == west.level)
            .then(|| self.skip_above_y.max(center.level))
    }
    fn sample(
        &self,
        field: AquiferField,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f64, AquiferError> {
        Ok(self.samplers.get(field).sample_value(context, position)? as f64)
    }
    fn surface_level(
        &mut self,
        context: &mut SamplerContext,
        x: i32,
        z: i32,
    ) -> Result<i32, AquiferError> {
        let x = (x >> 2).wrapping_shl(2);
        let z = (z >> 2).wrapping_shl(2);
        let key = ChunkPos::new(x, z).pack();
        if let Some(level) = self.surfaces.get(&key) {
            return Ok(*level);
        }
        let level = self
            .sample(AquiferField::SurfaceLevel, context, [x, 0, z])?
            .floor() as i32;
        self.surfaces.insert(key, level);
        Ok(level)
    }
    fn max_surface_level(
        &mut self,
        context: &mut SamplerContext,
        minimum: [i32; 2],
        maximum: [i32; 2],
    ) -> Result<i32, AquiferError> {
        let low = minimum.map(|v| v >> 2);
        let high = maximum.map(|v| v >> 2);
        let sizes: [i32; 2] = std::array::from_fn(|i| high[i].wrapping_sub(low[i]).wrapping_add(1));
        let volume = DensityVolume::new(
            [sizes[0], 1, sizes[1]],
            [low[0].wrapping_shl(2), 0, low[1].wrapping_shl(2)],
            [4, 1, 4],
        )?;
        if volume.size() > 2_097_152 {
            return Err(AquiferError::InvalidGrid);
        }
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(volume.size())
            .map_err(|_| AquiferError::Allocation)?;
        buffer.resize(volume.size(), 0.);
        self.samplers
            .get(AquiferField::SurfaceLevel)
            .sample_volume(context, &mut buffer, volume)?;
        self.surfaces
            .try_reserve(volume.size())
            .map_err(|_| AquiferError::Allocation)?;
        let mut maximum = i32::MIN;
        let mut index = 0;
        for z in 0..sizes[1] {
            for x in 0..sizes[0] {
                let value = (buffer[index] as f64).floor() as i32;
                index += 1;
                self.surfaces.insert(
                    ChunkPos::new(
                        low[0].wrapping_add(x).wrapping_shl(2),
                        low[1].wrapping_add(z).wrapping_shl(2),
                    )
                    .pack(),
                    value,
                );
                maximum = maximum.max(value);
            }
        }
        Ok(maximum)
    }
    fn grid_index(&self, position: [i32; 3]) -> Result<usize, AquiferError> {
        let relative: [i32; 3] = std::array::from_fn(|i| position[i].wrapping_sub(self.minimum[i]));
        // Vanilla checks the final array index, rather than each coordinate.
        let index = relative[1]
            .wrapping_mul(self.size_z)
            .wrapping_add(relative[2])
            .wrapping_mul(self.size_x)
            .wrapping_add(relative[0]);
        if index < 0 || index as usize >= self.locations.len() {
            return Err(AquiferError::OutsideGrid);
        }
        Ok(index as usize)
    }
    /// Loads the 12 cells around `grid` for the column `[x, z]`, drawing missing
    /// locations in vanilla's loop order. A cell outside the grid stops the
    /// loop where vanilla's index check would, and leaves no anchor cached.
    #[inline(never)]
    fn load_cells(&mut self, grid: [i32; 3], column: [i32; 2]) -> Result<(), AquiferError> {
        // The fill walks each column down: one anchor down, the 8 cells at the
        // old anchor's y and the y below stay. Their slots move up by one y
        // (2 slots); the 4 cells below are new, and vanilla's loop meets them
        // in the same order (slots 0, 1, 6, 7) with only drawn cells between.
        let down = self.anchor == Some([grid[0], grid[1].wrapping_add(1), grid[2]]);
        self.anchor = None;
        if down {
            let cells = &mut self.cells;
            for to in [11, 10, 9, 8, 5, 4, 3, 2] {
                cells.index[to] = cells.index[to - 2];
                cells.x[to] = cells.x[to - 2];
                cells.y[to] = cells.y[to - 2];
                cells.z[to] = cells.z[to - 2];
                cells.horizontal[to] = cells.horizontal[to - 2];
            }
            for (slot, dx, dz) in [(0, 0, 0), (1, 0, 1), (6, 1, 0), (7, 1, 1)] {
                self.load_cell(slot, [dx, -1, dz], grid, column)?;
            }
            if self.column != column {
                self.column = column;
                self.measure_column();
            }
        } else {
            let mut slot = 0;
            for dx in 0..=1i32 {
                for dy in -1..=1i32 {
                    for dz in 0..=1i32 {
                        self.load_cell(slot, [dx, dy, dz], grid, column)?;
                        slot += 1;
                    }
                }
            }
            self.column = column;
        }
        self.anchor = Some(grid);
        Ok(())
    }
    #[inline(always)]
    fn load_cell(
        &mut self,
        slot: usize,
        [dx, dy, dz]: [i32; 3],
        grid: [i32; 3],
        [x, z]: [i32; 2],
    ) -> Result<(), AquiferError> {
        let cell = [
            grid[0].wrapping_add(dx),
            grid[1].wrapping_add(dy),
            grid[2].wrapping_add(dz),
        ];
        let index = self.grid_index(cell)?;
        if self.locations[index] == i64::MAX {
            let mut random = self.random.at(BlockPos::new(cell[0], cell[1], cell[2]));
            let px = cell[0]
                .wrapping_shl(4)
                .wrapping_add(random.next_int_bound(10).expect("positive bound"));
            let py = cell[1]
                .wrapping_mul(12)
                .wrapping_add(random.next_int_bound(9).expect("positive bound"));
            let pz = cell[2]
                .wrapping_shl(4)
                .wrapping_add(random.next_int_bound(10).expect("positive bound"));
            let location = BlockPos::new(px, py, pz).pack();
            let point = BlockPos::unpack(location);
            self.locations[index] = location;
            self.points[index] = [point.x, point.y, point.z];
        }
        let [px, py, pz] = self.points[index];
        let (ax, az) = (px.wrapping_sub(x), pz.wrapping_sub(z));
        // The grid holds at most 16777216 cells, so indices fit.
        self.cells.index[slot] = index as u32;
        self.cells.x[slot] = px;
        self.cells.y[slot] = py;
        self.cells.z[slot] = pz;
        self.cells.horizontal[slot] = ax.wrapping_mul(ax).wrapping_add(az.wrapping_mul(az));
        Ok(())
    }
    /// Recomputes every cell's horizontal distance to `column`.
    fn measure_column(&mut self) {
        let [x, z] = self.column;
        for i in 0..12 {
            let ax = self.cells.x[i].wrapping_sub(x);
            let az = self.cells.z[i].wrapping_sub(z);
            self.cells.horizontal[i] = ax.wrapping_mul(ax).wrapping_add(az.wrapping_mul(az));
        }
    }
    #[inline(always)]
    fn status(
        &mut self,
        context: &mut SamplerContext,
        index: usize,
    ) -> Result<FluidStatus, AquiferError> {
        match self.statuses[index] {
            Some(status) => Ok(status),
            None => self.compute_status(context, index),
        }
    }
    #[inline(never)]
    fn compute_status(
        &mut self,
        context: &mut SamplerContext,
        index: usize,
    ) -> Result<FluidStatus, AquiferError> {
        let [x, y, z] = self.points[index];
        let status = self.compute_fluid(context, [x, y, z])?;
        self.statuses[index] = Some(status);
        Ok(status)
    }
    fn compute_fluid(
        &mut self,
        context: &mut SamplerContext,
        [x, y, z]: [i32; 3],
    ) -> Result<FluidStatus, AquiferError> {
        let global = self.picker.compute([x, y, z]);
        let upper = y.wrapping_add(12);
        let lower = y.wrapping_sub(12);
        let mut minimum = i32::MAX;
        let mut center_fluid = false;
        for [dx, dz] in SURFACE_OFFSETS {
            let sx = x.wrapping_add(dx.wrapping_mul(16));
            let sz = z.wrapping_add(dz.wrapping_mul(16));
            let surface = self.surface_level(context, sx, sz)?;
            let adjusted = surface.wrapping_add(8);
            let center = dx == 0 && dz == 0;
            if center && lower > adjusted {
                return Ok(global);
            }
            let above = upper > adjusted;
            if above || center {
                let status = self.picker.compute([sx, adjusted, sz]);
                if !self.blocks.is_air(status.at(adjusted, self.blocks.air())) {
                    if center {
                        center_fluid = true;
                    }
                    if above {
                        return Ok(status);
                    }
                }
            }
            minimum = minimum.min(surface);
        }
        let level =
            self.compute_surface_level(context, [x, y, z], global, minimum, center_fluid)?;
        let mut state = global.state;
        if level <= -10
            && level != WAY_BELOW_MIN_Y
            && state != self.blocks.lava()
            && self
                .sample(
                    AquiferField::Lava,
                    context,
                    [x.div_euclid(64), y.div_euclid(40), z.div_euclid(64)],
                )?
                .abs()
                > 0.3
        {
            state = self.blocks.lava();
        }
        Ok(FluidStatus { level, state })
    }
    fn compute_surface_level(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
        global: FluidStatus,
        minimum: i32,
        center_fluid: bool,
    ) -> Result<i32, AquiferError> {
        let [x, y, z] = position;
        let (dry, flooded) = if self.sample(AquiferField::Exclusion, context, position)? > 0. {
            (-1., -1.)
        } else {
            let distance = minimum.wrapping_add(8).wrapping_sub(y);
            let factor = if center_fluid {
                let alpha = (distance as f64 - 0.) / (64. - 0.);
                if alpha < 0. {
                    1.
                } else if alpha > 1. {
                    0.
                } else {
                    1. + alpha * (0. - 1.)
                }
            } else {
                0.
            };
            let value = self
                .sample(AquiferField::Floodedness, context, position)?
                .clamp(-1., 1.);
            let alpha = (factor - 1.) / (0. - 1.);
            let threshold_flooded = -0.3 + alpha * (0.8 - -0.3);
            let threshold_dry = -0.8 + alpha * (0.4 - -0.8);
            (value - threshold_dry, value - threshold_flooded)
        };
        if flooded > 0. {
            return Ok(global.level);
        }
        if dry > 0. {
            let spread = self.samplers.get(AquiferField::Spread).sample_value(
                context,
                [x.div_euclid(16), y.div_euclid(40), z.div_euclid(16)],
            )? * 10.;
            let offset = ((spread as f64 / 3.).floor() as i32).wrapping_mul(3);
            return Ok(minimum.min(
                y.div_euclid(40)
                    .wrapping_mul(40)
                    .wrapping_add(20)
                    .wrapping_add(offset),
            ));
        }
        Ok(WAY_BELOW_MIN_Y)
    }
    fn pressure(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
        memo: &mut f64,
        a: FluidStatus,
        b: FluidStatus,
    ) -> Result<f64, AquiferError> {
        let y = position[1];
        let sa = a.at(y, self.blocks.air());
        let sb = b.at(y, self.blocks.air());
        if (self.blocks.is_lava(sa) && self.blocks.is_water(sb))
            || (self.blocks.is_water(sa) && self.blocks.is_lava(sb))
        {
            return Ok(2.);
        }
        let gap = a.level.wrapping_sub(b.level).wrapping_abs();
        if gap == 0 {
            return Ok(0.);
        }
        let midpoint = 0.5 * a.level.wrapping_add(b.level) as f64;
        let dy = y as f64 + 0.5 - midpoint;
        let edge = gap as f64 / 2. - dy.abs();
        let q = if dy > 0. {
            let n = 0. + edge;
            if n > 0. { n / 1.5 } else { n / 2.5 }
        } else {
            let n = 3. + edge;
            if n > 0. { n / 3. } else { n / 10. }
        };
        let barrier = if q < -2. || q > 2. {
            0.
        } else {
            if memo.is_nan() {
                *memo = self.sample(AquiferField::Barrier, context, position)?;
            }
            *memo
        };
        Ok(2. * (barrier + q))
    }
    pub fn compute_substance(
        &mut self,
        context: &mut SamplerContext,
        [x, y, z]: [i32; 3],
        density: f64,
    ) -> Result<Option<BlockStateId>, AquiferError> {
        if density > 0. {
            self.schedule = false;
            return Ok(None);
        }
        let global = self.picker.compute([x, y, z]);
        if y > self.skip_above_y {
            self.schedule = false;
            return Ok(Some(global.at(y, self.blocks.air())));
        }
        if self.blocks.is_lava(global.at(y, self.blocks.air())) {
            self.schedule = false;
            return Ok(Some(self.blocks.lava()));
        }
        let (distances, indices) = self.closest([x, y, z])?;
        self.resolve(context, [x, y, z], density, distances, indices)
    }
    /// The squared distances to the 4 cell locations closest to the block, and
    /// their grid indices, closest first.
    #[inline(always)]
    fn closest(&mut self, [x, y, z]: [i32; 3]) -> Result<([i32; 4], [usize; 4]), AquiferError> {
        let grid = [
            x.wrapping_sub(5) >> 4,
            y.wrapping_add(1).div_euclid(12),
            z.wrapping_sub(5) >> 4,
        ];
        if self.anchor != Some(grid) {
            self.load_cells(grid, [x, z])?;
        } else if self.column != [x, z] {
            self.column = [x, z];
            self.measure_column();
        }
        let mut distances = [i32::MAX; 4];
        let mut indices = [0usize; 4];
        for i in 0..12 {
            let ay = self.cells.y[i].wrapping_sub(y);
            let distance = self.cells.horizontal[i].wrapping_add(ay.wrapping_mul(ay));
            let index = self.cells.index[i] as usize;
            // Equal distances insert before existing entries, as in vanilla.
            if distance <= distances[0] {
                distances = [distance, distances[0], distances[1], distances[2]];
                indices = [index, indices[0], indices[1], indices[2]];
            } else if distance <= distances[1] {
                distances[3] = distances[2];
                distances[2] = distances[1];
                distances[1] = distance;
                indices[3] = indices[2];
                indices[2] = indices[1];
                indices[1] = index;
            } else if distance <= distances[2] {
                distances[3] = distances[2];
                distances[2] = distance;
                indices[3] = indices[2];
                indices[2] = index;
            } else if distance <= distances[3] {
                distances[3] = distance;
                indices[3] = index;
            }
        }
        Ok((distances, indices))
    }
    /// The block's substance from its 4 closest cells.
    #[inline(always)]
    fn resolve(
        &mut self,
        context: &mut SamplerContext,
        [x, y, z]: [i32; 3],
        density: f64,
        distances: [i32; 4],
        indices: [usize; 4],
    ) -> Result<Option<BlockStateId>, AquiferError> {
        let a = self.status(context, indices[0])?;
        let s01 = similarity(distances[0], distances[1]);
        let candidate = a.at(y, self.blocks.air());
        if s01 <= 0. {
            self.schedule = if s01 >= FLOWING_UPDATE_SIMILARITY {
                a != self.status(context, indices[1])?
            } else {
                false
            };
            return Ok(Some(candidate));
        }
        if self.blocks.is_water(candidate)
            && self.blocks.is_lava(
                self.picker
                    .compute([x, y.wrapping_sub(1), z])
                    .at(y.wrapping_sub(1), self.blocks.air()),
            )
        {
            self.schedule = true;
            return Ok(Some(candidate));
        }
        let mut memo = f64::NAN;
        let b = self.status(context, indices[1])?;
        if density + s01 * self.pressure(context, [x, y, z], &mut memo, a, b)? > 0. {
            self.schedule = false;
            return Ok(None);
        }
        let c = self.status(context, indices[2])?;
        let s02 = similarity(distances[0], distances[2]);
        if s02 > 0.
            && density + s01 * s02 * self.pressure(context, [x, y, z], &mut memo, a, c)? > 0.
        {
            self.schedule = false;
            return Ok(None);
        }
        let s12 = similarity(distances[1], distances[2]);
        if s12 > 0.
            && density + s01 * s12 * self.pressure(context, [x, y, z], &mut memo, b, c)? > 0.
        {
            self.schedule = false;
            return Ok(None);
        }
        self.schedule = a != b
            || (s12 >= FLOWING_UPDATE_SIMILARITY && b != c)
            || (s02 >= FLOWING_UPDATE_SIMILARITY && a != c)
            || (s02 >= FLOWING_UPDATE_SIMILARITY
                && similarity(distances[0], distances[3]) >= FLOWING_UPDATE_SIMILARITY
                && a != self.status(context, indices[3])?);
        Ok(Some(candidate))
    }
}
fn similarity(a: i32, b: i32) -> f64 {
    1. - b.wrapping_sub(a) as f64 / 25.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::Tag;
    use crate::worldgen::aquifer::GlobalFluidPicker;
    use horizon_core::{density::DensitySampler, random::LegacyPositionalRandom};
    use std::sync::Arc;

    /// States as `(name, level)`; each name's first state is its default.
    fn registry(states: &[(&str, &str)]) -> BlockRegistry {
        let mut defaults = Vec::new();
        for (id, (name, _)) in states.iter().enumerate() {
            if !defaults.iter().any(|(n, _)| n == name) {
                defaults.push((*name, Tag::Int(id as i32)));
            }
        }
        let states = states.iter().map(|(name, level)| {
            let id = ("id", Tag::String((*name).into()));
            match *level {
                "" => Tag::compound([id]),
                level => Tag::compound([
                    id,
                    (
                        "properties",
                        Tag::compound([("level", Tag::String(level.into()))]),
                    ),
                ]),
            }
        });
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(crate::DATA_VERSION)),
            ("states", Tag::List(states.collect())),
            ("defaults", Tag::compound(defaults)),
        ]))
        .unwrap()
    }
    const STATES: [(&str, &str); 10] = [
        ("minecraft:stone", ""),
        ("minecraft:water", "0"),
        ("minecraft:water", "3"),
        ("minecraft:lava", "0"),
        ("minecraft:lava", "5"),
        ("minecraft:air", ""),
        ("minecraft:cave_air", ""),
        ("minecraft:void_air", ""),
        ("minecraft:lava_cauldron", ""),
        ("minecraft:waterlogged", ""),
    ];

    /// The block classification before its default-state shortcuts: every
    /// state reads its name.
    struct NameBlocks<'a>(RegistryAquiferBlockStates<'a>);
    impl AquiferBlockStates for NameBlocks<'_> {
        fn air(&self) -> BlockStateId {
            self.0.air
        }
        fn lava(&self) -> BlockStateId {
            self.0.lava
        }
        fn is_air(&self, state: BlockStateId) -> bool {
            matches!(
                self.0.name(state),
                Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
            )
        }
        fn is_lava(&self, state: BlockStateId) -> bool {
            self.0.name(state) == Some("minecraft:lava")
        }
        fn is_water(&self, state: BlockStateId) -> bool {
            self.0.name(state) == Some("minecraft:water")
        }
    }

    #[test]
    fn block_classification_matches_the_names() {
        // With and without a water default. IDs past the end name no block.
        let no_water = [
            ("minecraft:air", ""),
            ("minecraft:lava", ""),
            ("minecraft:stone", ""),
        ];
        for registry in [registry(&STATES), registry(&no_water)] {
            let fast = RegistryAquiferBlockStates::new(&registry).unwrap();
            let names = NameBlocks(RegistryAquiferBlockStates::new(&registry).unwrap());
            for id in 0..registry.len() as u32 + 3 {
                let state = BlockStateId(id);
                assert_eq!(fast.is_air(state), names.is_air(state), "{id}");
                assert_eq!(fast.is_lava(state), names.is_lava(state), "{id}");
                assert_eq!(fast.is_water(state), names.is_water(state), "{id}");
            }
        }
    }

    /// A cheap, position-dependent stand-in for a noise root, spread over
    /// `base .. base + span`, so every aquifer branch is reached.
    #[derive(Debug)]
    struct Pattern {
        salt: u32,
        base: f32,
        span: f32,
    }
    impl DensitySampler for Pattern {
        fn sample_value(
            &self,
            _: &mut SamplerContext,
            [x, y, z]: [i32; 3],
        ) -> Result<f32, SampleError> {
            Ok(self.base + hash(self.salt, x, y, z) as f32 / 16_777_216. * self.span)
        }
    }
    fn hash(salt: u32, x: i32, y: i32, z: i32) -> u32 {
        let mut h = (x as u32).wrapping_mul(0x9e37_79b9)
            ^ (y as u32).wrapping_mul(0x85eb_ca6b)
            ^ (z as u32).wrapping_mul(0xc2b2_ae35)
            ^ salt.wrapping_mul(0x2718_2818);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2545_f491);
        h ^= h >> 13;
        h >> 8
    }
    fn samplers() -> AquiferSamplers {
        let pattern =
            |salt, base, span| Arc::new(Pattern { salt, base, span }) as Arc<dyn DensitySampler>;
        // AquiferField::ALL order: barrier, floodedness, spread, lava,
        // exclusion, surface level. Surfaces from deep below the lava level
        // to above sea level, so cells reach every fluid rule.
        AquiferSamplers::new([
            pattern(1, -1.5, 3.),
            pattern(2, -1.2, 2.4),
            pattern(3, -1., 2.),
            pattern(4, -0.6, 1.2),
            pattern(5, -1., 1.3),
            pattern(6, -90., 190.),
        ])
    }

    /// The closest-cell search before the cell arrays, kept verbatim: a
    /// neighbourhood of packed locations, a column cache and a two-array
    /// insertion.
    #[derive(Default)]
    struct Previous {
        neighborhood: Option<([i32; 3], [(usize, BlockPos); 12])>,
        cached_col: [i32; 2],
        cached_y_anchor: i32,
        cached_h: [i32; 12],
        cached_py: [i32; 12],
        cached_indices: [usize; 12],
    }
    type NameAquifer<'a> =
        NoiseBasedAquifer<GlobalFluidPicker, LegacyPositionalRandom, NameBlocks<'a>>;
    impl Previous {
        fn compute_substance(
            &mut self,
            this: &mut NameAquifer<'_>,
            context: &mut SamplerContext,
            [x, y, z]: [i32; 3],
            density: f64,
        ) -> Result<Option<BlockStateId>, AquiferError> {
            if density > 0. {
                this.schedule = false;
                return Ok(None);
            }
            let global = this.picker.compute([x, y, z]);
            if y > this.skip_above_y {
                this.schedule = false;
                return Ok(Some(global.at(y, this.blocks.air())));
            }
            if this.blocks.is_lava(global.at(y, this.blocks.air())) {
                this.schedule = false;
                return Ok(Some(this.blocks.lava()));
            }
            let grid = [
                x.wrapping_sub(5) >> 4,
                y.wrapping_add(1).div_euclid(12),
                z.wrapping_sub(5) >> 4,
            ];
            if self.neighborhood.as_ref().map(|(key, _)| *key) != Some(grid) {
                let mut entries = [(0, BlockPos::default()); 12];
                let mut next = 0;
                for dx in 0..=1i32 {
                    for dy in -1..=1i32 {
                        for dz in 0..=1i32 {
                            let cell = [
                                grid[0].wrapping_add(dx),
                                grid[1].wrapping_add(dy),
                                grid[2].wrapping_add(dz),
                            ];
                            let index = this.grid_index(cell)?;
                            if this.locations[index] == i64::MAX {
                                let mut random =
                                    this.random.at(BlockPos::new(cell[0], cell[1], cell[2]));
                                let px = cell[0].wrapping_shl(4).wrapping_add(
                                    random.next_int_bound(10).expect("positive bound"),
                                );
                                let py = cell[1].wrapping_mul(12).wrapping_add(
                                    random.next_int_bound(9).expect("positive bound"),
                                );
                                let pz = cell[2].wrapping_shl(4).wrapping_add(
                                    random.next_int_bound(10).expect("positive bound"),
                                );
                                this.locations[index] = BlockPos::new(px, py, pz).pack();
                                // The status cache reads the unpacked location.
                                let point = BlockPos::unpack(this.locations[index]);
                                this.points[index] = [point.x, point.y, point.z];
                            }
                            entries[next] = (index, BlockPos::unpack(this.locations[index]));
                            next += 1;
                        }
                    }
                }
                self.neighborhood = Some((grid, entries));
            }
            if x != self.cached_col[0] || z != self.cached_col[1] || grid[1] != self.cached_y_anchor
            {
                self.cached_col = [x, z];
                self.cached_y_anchor = grid[1];
                let entries = &self
                    .neighborhood
                    .as_ref()
                    .expect("populated neighborhood")
                    .1;
                for i in 0..12 {
                    let (index, point) = entries[i];
                    let ax = point.x.wrapping_sub(x);
                    let az = point.z.wrapping_sub(z);
                    self.cached_indices[i] = index;
                    self.cached_h[i] = ax.wrapping_mul(ax).wrapping_add(az.wrapping_mul(az));
                    self.cached_py[i] = point.y;
                }
            }
            let mut distances = [i32::MAX; 4];
            let mut indices = [0usize; 4];
            for i in 0..12 {
                let ay = self.cached_py[i].wrapping_sub(y);
                let distance = self.cached_h[i].wrapping_add(ay.wrapping_mul(ay));
                let index = self.cached_indices[i];
                if distance <= distances[0] {
                    distances = [distance, distances[0], distances[1], distances[2]];
                    indices = [index, indices[0], indices[1], indices[2]];
                } else if distance <= distances[1] {
                    distances[3] = distances[2];
                    distances[2] = distances[1];
                    distances[1] = distance;
                    indices[3] = indices[2];
                    indices[2] = indices[1];
                    indices[1] = index;
                } else if distance <= distances[2] {
                    distances[3] = distances[2];
                    distances[2] = distance;
                    indices[3] = indices[2];
                    indices[2] = index;
                } else if distance <= distances[3] {
                    distances[3] = distance;
                    indices[3] = index;
                }
            }
            this.resolve(context, [x, y, z], density, distances, indices)
        }
    }

    /// The cell arrays, the inlined status cache and the default-state
    /// classification answer what the code before them answered, bit for bit:
    /// the same block (or default block), the same pending fluid update and the
    /// same errors, then the same cached locations, statuses and surfaces.
    /// Columns are swept top-down as the noise fill does (ocean water, the
    /// lava level below -54, cave air with barriers between aquifers), then
    /// blocks are asked in a scrambled order, then outside the grid.
    #[test]
    fn substance_matches_the_previous_code_bit_for_bit() {
        let registry = registry(&STATES);
        let air = registry.default_state("minecraft:air").unwrap();
        let water = registry.default_state("minecraft:water").unwrap();
        let lava = registry.default_state("minecraft:lava").unwrap();
        let picker = GlobalFluidPicker::new(63, water, lava, air);
        for (seed, chunk) in [(7, [0, 0]), (-3, [-5, 12]), (12345, [31, -32])] {
            let volume =
                DensityVolume::new([16, 384, 16], [chunk[0] * 16, -64, chunk[1] * 16], [1; 3])
                    .unwrap();
            let mut context = SamplerContext::cached();
            let mut candidate = NoiseBasedAquifer::new(
                samplers(),
                picker,
                LegacyPositionalRandom::new(seed),
                RegistryAquiferBlockStates::new(&registry).unwrap(),
                volume,
                &mut context,
            )
            .unwrap();
            let mut reference = NoiseBasedAquifer::new(
                samplers(),
                picker,
                LegacyPositionalRandom::new(seed),
                NameBlocks(RegistryAquiferBlockStates::new(&registry).unwrap()),
                volume,
                &mut context,
            )
            .unwrap();
            let mut previous = Previous {
                cached_col: [i32::MIN, i32::MIN],
                cached_y_anchor: i32::MIN,
                ..Previous::default()
            };
            assert_eq!(
                candidate.skip_sampling_above_y(),
                reference.skip_sampling_above_y()
            );
            let [bx, bz] = [chunk[0] * 16, chunk[1] * 16];
            let mut positions = Vec::new();
            for z in 0..16 {
                for x in 0..16 {
                    for y in (-64..320).rev() {
                        positions.push([bx + x, y, bz + z]);
                    }
                }
            }
            positions.extend((0..20_000).map(|i| {
                let h = hash(seed as u32, i, 0, 0);
                let y = -64 + (hash(9, i, 1, 0) % 384) as i32;
                [bx + (h & 15) as i32, y, bz + ((h >> 4) & 15) as i32]
            }));
            // Outside the grid: errors, then the cache must still answer.
            // Vanilla checks only the final index, so some of these wrap onto
            // other rows of the grid instead of failing.
            positions.extend([
                [bx + 100_000, 0, bz],
                [bx + 3, -40, bz + 3],
                [bx, 10, bz - 100_000],
                [bx + 40, 10, bz],
                [bx - 100, 10, bz - 100],
                [bx + 4, -41, bz + 4],
                [bx + 4, -42, bz + 4],
            ]);
            let mut outcomes = [0usize; 6];
            for position in positions {
                let [x, y, z] = position;
                // Mostly open space, some solid, a few NaN: barriers decide
                // where the density is just below zero.
                let h = hash(seed as u32 ^ 77, x, y, z);
                let density = match h % 64 {
                    0 => f64::NAN,
                    1..=6 => 0.2,
                    _ => -f64::from(h % 1000) / 1500.,
                };
                let got = candidate.compute_substance(&mut context, position, density);
                let want =
                    previous.compute_substance(&mut reference, &mut context, position, density);
                match (&got, &want) {
                    (Ok(got), Ok(want)) => assert_eq!(got, want, "{position:?} {density}"),
                    (Err(got), Err(want)) => {
                        assert_eq!(got.to_string(), want.to_string(), "{position:?}")
                    }
                    _ => panic!("{position:?} {density}: {got:?} vs {want:?}"),
                }
                assert_eq!(
                    candidate.should_schedule_fluid_update(),
                    reference.should_schedule_fluid_update(),
                    "{position:?} {density}"
                );
                // Errors, barriers, air, water, lava, pending updates.
                let seen = [
                    got.is_err(),
                    density <= 0. && matches!(got, Ok(None)),
                    matches!(got, Ok(Some(s)) if s == air),
                    matches!(got, Ok(Some(s)) if s == water),
                    matches!(got, Ok(Some(s)) if s == lava),
                    candidate.should_schedule_fluid_update(),
                ];
                for (count, seen) in outcomes.iter_mut().zip(seen) {
                    *count += usize::from(seen);
                }
            }
            assert!(outcomes.iter().all(|&n| n > 0), "{outcomes:?}");
            assert!(candidate.cached_cells().eq(reference.cached_cells()));
            for (location, point) in candidate.locations.iter().zip(&candidate.points) {
                if *location != i64::MAX {
                    let unpacked = BlockPos::unpack(*location);
                    assert_eq!(*point, [unpacked.x, unpacked.y, unpacked.z]);
                }
            }
            let mut surfaces: Vec<_> = candidate.cached_surfaces().collect();
            let mut want: Vec<_> = reference.cached_surfaces().collect();
            surfaces.sort();
            want.sort();
            assert_eq!(surfaces, want);
        }
    }
}
