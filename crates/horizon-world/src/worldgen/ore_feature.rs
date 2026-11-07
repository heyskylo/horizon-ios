//! Ore placement with ordered replacement rules and air-exposure checks.
use super::{
    DecodeError, Object,
    block_feature::{BlockReplacements, FeatureBlockBindings, decode_targets},
    density::identifier,
    feature::{FeatureError, FeatureWorld},
    feature_graph::FeatureExecutionError,
    int_range, number_text,
    placement::PlacementContext,
    required,
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::{geometry::Direction, random::RandomSource};

#[derive(Clone, Debug)]
pub struct ScatteredOreFeature {
    targets: BlockReplacements,
    size: i32,
    discard: f32,
    air: [BlockStateId; 3],
}
impl ScatteredOreFeature {
    pub fn from_json(json: &str, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        Self::decode(json, bindings, 0, &mut 65536)
    }
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let kind = identifier(&serde_json::from_str::<String>(
            required(&object, "type")?.get(),
        )?)?;
        if kind != "minecraft:scattered_ore" {
            return Err(DecodeError::Field("scattered ore type"));
        }
        let targets = decode_targets(&object, bindings, depth, remaining)?;
        let size = int_range(required(&object, "size")?, "ore size", 0, 64)?;
        let discard: f32 = number_text(required(&object, "discard_chance_on_air_exposure")?)?
            .parse()
            .map_err(|_| DecodeError::Field("ore air exposure"))?;
        if !(0.0..=1.0).contains(&discard) || discard.is_sign_negative() {
            return Err(DecodeError::Field("ore air exposure"));
        }
        Ok(Self {
            targets,
            size,
            discard,
            air: bindings.air,
        })
    }
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: [i32; 3],
    ) -> Result<bool, FeatureError<W::Error>> {
        let count = random
            .next_int_bound(self.size + 1)
            .map_err(FeatureError::Random)?;
        for attempt in 0..count {
            let spread = attempt.min(7) as f32;
            let position = origin.map(|component| {
                let offset = (random.next_float() - random.next_float()) * spread;
                // Widen before adding 0.5 so a float just below a tie cannot
                // round up during addition. Values are bounded by +/-7.
                let offset = (f64::from(offset) + 0.5).floor() as i32;
                component.wrapping_add(offset)
            });
            let current = world.block(position).map_err(FeatureError::World)?;
            for (rule, state) in &self.targets {
                if !rule.test(current, position, random) {
                    continue;
                }
                let skip_air = self.discard <= 0.0
                    || (self.discard < 1.0 && random.next_float() >= self.discard);
                if !skip_air {
                    let mut exposed = false;
                    for direction in Direction::ALL {
                        let delta = direction.normal();
                        let neighbor = std::array::from_fn(|i| position[i].wrapping_add(delta[i]));
                        if self
                            .air
                            .contains(&world.block(neighbor).map_err(FeatureError::World)?)
                        {
                            exposed = true;
                            break;
                        }
                    }
                    if exposed {
                        continue;
                    }
                }
                world
                    .set_block(position, *state, 2)
                    .map_err(FeatureError::World)?;
                break;
            }
        }
        Ok(true)
    }
}

/// Vanilla `minecraft:ore`: an interpolated chain of spheres written straight
/// into chunk sections (BulkSectionAccess), bypassing block update effects.
#[derive(Clone, Debug)]
pub struct OreFeature {
    targets: BlockReplacements,
    size: i32,
    discard: f32,
    air: [BlockStateId; 3],
    /// When no target rule draws from the random source (and there are at
    /// most 32 targets): every Y range the rules test. Which targets match
    /// is then a function of the state and the ranges holding Y.
    heights: Option<Vec<(i32, i32)>>,
}
impl OreFeature {
    pub fn from_json(json: &str, bindings: &FeatureBlockBindings<'_>) -> Result<Self, DecodeError> {
        Self::decode(json, bindings, 0, &mut 65536)
    }
    pub(crate) fn decode(
        json: &str,
        bindings: &FeatureBlockBindings<'_>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let kind = identifier(&serde_json::from_str::<String>(
            required(&object, "type")?.get(),
        )?)?;
        if kind != "minecraft:ore" {
            return Err(DecodeError::Field("ore type"));
        }
        let targets = decode_targets(&object, bindings, depth, remaining)?;
        let size = int_range(required(&object, "size")?, "ore size", 0, 64)?;
        let discard: f32 = number_text(required(&object, "discard_chance_on_air_exposure")?)?
            .parse()
            .map_err(|_| DecodeError::Field("ore air exposure"))?;
        if !(0.0..=1.0).contains(&discard) || discard.is_sign_negative() {
            return Err(DecodeError::Field("ore air exposure"));
        }
        let mut heights = Vec::new();
        let pure = targets
            .iter()
            .try_for_each(|(rule, _)| rule.pure_heights(&mut heights));
        let heights =
            (pure.is_some() && targets.len() <= 32 && heights.len() <= 64).then_some(heights);
        Ok(Self {
            targets,
            size,
            discard,
            air: bindings.air,
            heights,
        })
    }
    pub fn place<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: [i32; 3],
    ) -> Result<bool, FeatureExecutionError<W::Error>> {
        let dir = random.next_float() * std::f32::consts::PI;
        let spread_xy = self.size as f32 / 8.0;
        let max_radius = ((self.size as f32 / 16.0 * 2.0 + 1.0) / 2.0).ceil() as i32;
        let (ox, oz) = (f64::from(origin[0]), f64::from(origin[2]));
        let spread = f64::from(spread_xy);
        // The blob's ends, needed only once a column under it is found
        // (most attempts high in To The Max's tall range find none).
        let ends = || {
            let (sin, cos) = (
                horizon_core::jmath::sin(f64::from(dir)),
                horizon_core::jmath::cos(f64::from(dir)),
            );
            [
                ox + sin * spread,
                ox - sin * spread,
                oz + cos * spread,
                oz - cos * spread,
            ]
        };
        let y0 = f64::from(
            origin[1]
                .wrapping_add(random.next_int_bound(3)?)
                .wrapping_sub(2),
        );
        let y1 = f64::from(
            origin[1]
                .wrapping_add(random.next_int_bound(3)?)
                .wrapping_sub(2),
        );
        let spread_ceil = spread.ceil() as i32;
        let x_start = origin[0].wrapping_sub(spread_ceil).wrapping_sub(max_radius);
        let y_start = origin[1].wrapping_sub(2).wrapping_sub(max_radius);
        let z_start = origin[2].wrapping_sub(spread_ceil).wrapping_sub(max_radius);
        let size_xz = 2 * (spread_ceil + max_radius);
        let size_y = 2 * (2 + max_radius);
        if let Some(found) = world
            .any_height_at_least(
                HeightmapType::OceanFloorWg,
                [x_start, z_start],
                size_xz,
                y_start,
            )
            .map_err(FeatureExecutionError::Query)?
        {
            return match found {
                true => self.do_place(
                    world,
                    random,
                    ends(),
                    [y0, y1],
                    [x_start, y_start, z_start],
                    size_xz,
                    size_y,
                ),
                false => Ok(false),
            };
        }
        for x in 0..=size_xz {
            for z in 0..=size_xz {
                let (px, pz) = (x_start.wrapping_add(x), z_start.wrapping_add(z));
                let floor = world
                    .height(HeightmapType::OceanFloorWg, px, pz)
                    .map_err(FeatureExecutionError::Query)?;
                if y_start <= floor {
                    return self.do_place(
                        world,
                        random,
                        ends(),
                        [y0, y1],
                        [x_start, y_start, z_start],
                        size_xz,
                        size_y,
                    );
                }
            }
        }
        Ok(false)
    }
    #[allow(clippy::too_many_arguments)]
    fn do_place<W: FeatureWorld + PlacementContext>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        [x0, x1, z0, z1]: [f64; 4],
        [y0, y1]: [f64; 2],
        [x_start, y_start, z_start]: [i32; 3],
        size_xz: i32,
        size_y: i32,
    ) -> Result<bool, FeatureExecutionError<W::Error>> {
        let size = self.size as usize;
        let mut data = vec![[0f64; 4]; size];
        for (i, entry) in data.iter_mut().enumerate() {
            let step = i as f32 / self.size as f32;
            let step64 = f64::from(step);
            let xx = x0 + step64 * (x1 - x0);
            let yy = y0 + step64 * (y1 - y0);
            let zz = z0 + step64 * (z1 - z0);
            let ss = random.next_double() * f64::from(self.size) / 16.0;
            let sine = horizon_core::geometry::trig::sin(f64::from(std::f32::consts::PI * step));
            let r = (f64::from(sine + 1.0) * ss + 1.0) / 2.0;
            *entry = [xx, yy, zz, r];
        }
        for i1 in 0..size.saturating_sub(1) {
            if data[i1][3] <= 0.0 {
                continue;
            }
            for i2 in i1 + 1..size {
                if data[i2][3] <= 0.0 {
                    continue;
                }
                let dx = data[i1][0] - data[i2][0];
                let dy = data[i1][1] - data[i2][1];
                let dz = data[i1][2] - data[i2][2];
                let dr = data[i1][3] - data[i2][3];
                if dr * dr > dx * dx + dy * dy + dz * dz {
                    if dr > 0.0 {
                        data[i2][3] = -1.0;
                    } else {
                        data[i1][3] = -1.0;
                    }
                }
            }
        }
        let tested_len = (size_xz as usize) * (size_y as usize) * (size_xz as usize);
        let mut tested = vec![0u64; tested_len.div_ceil(64)];
        let mut placed = 0;
        let mut matches = TargetMemo::default();
        let mut squares = SphereSquares::default();
        for sphere in data {
            if sphere[3] < 0.0 {
                continue;
            }
            squares.each_cell(sphere, [x_start, y_start, z_start], |[x, y, z]| {
                if world
                    .outside_build_height(y)
                    .map_err(FeatureExecutionError::Query)?
                {
                    return Ok(());
                }
                let index = i64::from(x.wrapping_sub(x_start))
                    + i64::from(y.wrapping_sub(y_start)) * i64::from(size_xz)
                    + i64::from(z.wrapping_sub(z_start)) * i64::from(size_xz) * i64::from(size_y);
                // The sphere bounds keep indices inside the native BitSet.
                let Some(index) = usize::try_from(index).ok().filter(|&i| i < tested_len) else {
                    return Err(FeatureExecutionError::WorkLimit);
                };
                let (word, bit) = (&mut tested[index / 64], 1u64 << (index % 64));
                if *word & bit != 0 {
                    return Ok(());
                }
                *word |= bit;
                let p = [x, y, z];
                if !world
                    .ensure_can_write(p)
                    .map_err(FeatureExecutionError::Query)?
                {
                    return Ok(());
                }
                let Some(state) = world
                    .section_block(p)
                    .map_err(FeatureExecutionError::Query)?
                else {
                    return Ok(());
                };
                let matched = match &self.heights {
                    Some(heights) => {
                        let ranges = heights.iter().enumerate().fold(0u64, |m, (i, &(lo, hi))| {
                            m | u64::from(lo <= p[1] && p[1] <= hi) << i
                        });
                        matches.get(state, ranges, || {
                            self.targets
                                .iter()
                                .enumerate()
                                .fold(0u32, |m, (i, (rule, _))| {
                                    m | u32::from(rule.test(state, p, random)) << i
                                })
                        })
                    }
                    None => u32::MAX,
                };
                for (i, (rule, target)) in self.targets.iter().enumerate() {
                    if self.heights.is_some() {
                        if matched >> i & 1 == 0 {
                            continue;
                        }
                    } else if !rule.test(state, p, random) {
                        continue;
                    }
                    if !skip_air_check(random, self.discard) {
                        let mut exposed = false;
                        for direction in Direction::ALL {
                            let n = direction.normal();
                            let q = std::array::from_fn(|i| p[i].wrapping_add(n[i]));
                            let neighbor = world
                                .section_block(q)
                                .map_err(FeatureExecutionError::Query)?
                                .unwrap_or(self.air[0]);
                            if self.air.contains(&neighbor) {
                                exposed = true;
                                break;
                            }
                        }
                        if exposed {
                            continue;
                        }
                    }
                    world
                        .set_section_block(p, *target)
                        .map_err(FeatureExecutionError::Query)?;
                    placed += 1;
                    break;
                }
                Ok(())
            })?;
        }
        Ok(placed > 0)
    }
}
/// The cells OreFeature.doPlace tests for one sphere `[x, y, z, r]` of a
/// blob: those of the sphere's box (clamped below to the blob's `start`)
/// whose `xd * xd + yd * yd + zd * zd` is under 1, in vanilla's x, y, z order.
///
/// Along one axis, `d * d` depends on that coordinate alone, so each axis's
/// squares are worked out once per sphere with vanilla's expression, and
/// every sum is bit for bit vanilla's. Along an axis the squares fall and then
/// rise (each step rounds a monotonic function, as does adding a fixed sum),
/// so the cells of a row that pass form one run around the smallest square:
/// it is found from there outwards, and the cells outside the sphere are
/// never visited.
#[derive(Default)]
struct SphereSquares {
    axes: [Vec<f64>; 3],
}
impl SphereSquares {
    fn each_cell<E>(
        &mut self,
        [xx, yy, zz, r]: [f64; 4],
        start: [i32; 3],
        mut cell: impl FnMut([i32; 3]) -> Result<(), E>,
    ) -> Result<(), E> {
        let centre = [xx, yy, zz];
        let mut from = [0; 3];
        let mut lowest = [0; 3];
        for axis in 0..3 {
            let low = ((centre[axis] - r).floor() as i32).max(start[axis]);
            let high = ((centre[axis] + r).floor() as i32).max(low);
            let squares = &mut self.axes[axis];
            squares.clear();
            squares.extend((low..=high).map(|v| {
                let d = (f64::from(v) + 0.5 - centre[axis]) / r;
                d * d
            }));
            from[axis] = low;
            lowest[axis] = (0..squares.len())
                .min_by(|&a, &b| squares[a].total_cmp(&squares[b]))
                .unwrap_or(0);
        }
        let [xs, ys, zs] = &self.axes;
        // Vanilla's `xd * xd >= 1.0` is `0.0 + xd * xd >= 1.0`: a square is
        // never negative zero, and adding zero to it rounds nothing.
        let Some((x_low, x_high)) = pass_run(xs, lowest[0], 0.0) else {
            return Ok(());
        };
        for i in x_low..=x_high {
            let xd2 = xs[i];
            let Some((y_low, y_high)) = pass_run(ys, lowest[1], xd2) else {
                continue;
            };
            for j in y_low..=y_high {
                let xy = xd2 + ys[j];
                let Some((z_low, z_high)) = pass_run(zs, lowest[2], xy) else {
                    continue;
                };
                for k in z_low..=z_high {
                    cell([from[0] + i as i32, from[1] + j as i32, from[2] + k as i32])?;
                }
            }
        }
        Ok(())
    }
}
/// The run of `squares` around index `lowest` (a smallest one) where vanilla's
/// `base + square >= 1.0` fails, or None when even the smallest reaches 1.
/// The sums are never NaN (a sphere's radius is at least 0.5), so failing
/// that test is being under 1.
fn pass_run(squares: &[f64], lowest: usize, base: f64) -> Option<(usize, usize)> {
    let passes = |i: usize| base + squares[i] < 1.0;
    if !passes(lowest) {
        return None;
    }
    let (mut low, mut high) = (lowest, lowest);
    while low > 0 && passes(low - 1) {
        low -= 1;
    }
    while high + 1 < squares.len() && passes(high + 1) {
        high += 1;
    }
    Some((low, high))
}
/// Recent (state, Y ranges) -> matching targets of a pure-rule ore.
#[derive(Default)]
struct TargetMemo {
    entries: [(u32, u64, u32); 8],
    len: usize,
    next: usize,
}
impl TargetMemo {
    fn get(&mut self, state: BlockStateId, ranges: u64, compute: impl FnOnce() -> u32) -> u32 {
        let key = (state.0, ranges);
        if let Some(e) = self.entries[..self.len].iter().find(|e| (e.0, e.1) == key) {
            return e.2;
        }
        let value = compute();
        self.entries[self.next] = (key.0, key.1, value);
        self.next = (self.next + 1) % self.entries.len();
        self.len = (self.len + 1).min(self.entries.len());
        value
    }
}
fn skip_air_check(random: &mut impl RandomSource, discard: f32) -> bool {
    if discard <= 0.0 {
        true
    } else if discard >= 1.0 {
        false
    } else {
        random.next_float() >= discard
    }
}

#[cfg(test)]
mod tests {
    use super::SphereSquares;

    /// OreFeature.doPlace's loops over one sphere's whole box, as vanilla
    /// writes them: the cells it goes on to test.
    fn box_scan([xx, yy, zz, r]: [f64; 4], [x_start, y_start, z_start]: [i32; 3]) -> Vec<[i32; 3]> {
        let x_min = ((xx - r).floor() as i32).max(x_start);
        let y_min = ((yy - r).floor() as i32).max(y_start);
        let z_min = ((zz - r).floor() as i32).max(z_start);
        let x_max = ((xx + r).floor() as i32).max(x_min);
        let y_max = ((yy + r).floor() as i32).max(y_min);
        let z_max = ((zz + r).floor() as i32).max(z_min);
        let mut cells = Vec::new();
        for x in x_min..=x_max {
            let xd = (f64::from(x) + 0.5 - xx) / r;
            if xd * xd >= 1.0 {
                continue;
            }
            for y in y_min..=y_max {
                let yd = (f64::from(y) + 0.5 - yy) / r;
                if xd * xd + yd * yd >= 1.0 {
                    continue;
                }
                for z in z_min..=z_max {
                    let zd = (f64::from(z) + 0.5 - zz) / r;
                    if xd * xd + yd * yd + zd * zd >= 1.0 {
                        continue;
                    }
                    cells.push([x, y, z]);
                }
            }
        }
        cells
    }

    #[test]
    fn a_sphere_visits_exactly_the_cells_vanillas_box_scan_tests() {
        // SplitMix64: any fixed spread of spheres will do.
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut unit = move || {
            seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = seed;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            ((z ^ (z >> 31)) >> 11) as f64 / (1_u64 << 53) as f64
        };
        let mut squares = SphereSquares::default();
        let mut visited = 0;
        for _ in 0..20_000 {
            // Centres and radii on whole and half blocks put cells exactly on
            // the sphere's edge, where a sum is exactly 1.
            let mut sphere = [0.0; 4];
            let r = 0.5 + unit() * 4.0;
            sphere[3] = if unit() < 0.3 {
                (r * 2.0).round() / 2.0
            } else {
                r
            };
            let mut start = [0; 3];
            for axis in 0..3 {
                let c = (unit() - 0.5) * 80_000.0;
                sphere[axis] = match (unit() * 3.0) as u32 {
                    0 => c.floor(),
                    1 => c.floor() + 0.5,
                    _ => c,
                };
                // Sometimes the blob's start cuts the sphere's box.
                start[axis] = (sphere[axis] - sphere[3]).floor() as i32 + (unit() * 5.0) as i32 - 1;
            }
            let mut cells = Vec::new();
            squares
                .each_cell::<()>(sphere, start, |p| {
                    cells.push(p);
                    Ok(())
                })
                .unwrap();
            assert_eq!(
                cells,
                box_scan(sphere, start),
                "sphere {sphere:?}, start {start:?}"
            );
            visited += cells.len();
        }
        assert!(visited > 100_000, "only {visited} cells");
    }
}
