use super::*;

/// Interpolation over a coarse density grid. Scalar queries and volume queries
/// retain vanilla's distinct floating-point paths and child bulk calls.
#[derive(Clone, Debug)]
pub struct InterpolatedSampler {
    input: Arc<dyn DensitySampler>,
    cell_size: [i32; 3],
    inverse: [f32; 3],
}
impl InterpolatedSampler {
    pub fn new(
        input: Arc<dyn DensitySampler>,
        cell_size_xz: i32,
        cell_size_y: i32,
    ) -> Result<Self, SampleError> {
        if cell_size_xz <= 0 || cell_size_y <= 0 {
            return Err(DensityError::NonpositiveSteps.into());
        }
        let cell_size = [cell_size_xz, cell_size_y, cell_size_xz];
        Ok(Self {
            input,
            cell_size,
            inverse: cell_size.map(|size| 1.0 / size as f32),
        })
    }

    fn sample_block_volume(
        &self,
        context: &mut SamplerContext,
        output: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        let minimum = volume.minimum();
        let maximum = volume.maximum();
        let first: [i32; 3] = std::array::from_fn(|i| minimum[i].div_euclid(self.cell_size[i]));
        let counts: [i32; 3] = std::array::from_fn(|i| {
            maximum[i]
                .div_euclid(self.cell_size[i])
                .wrapping_sub(first[i])
                .wrapping_add(1)
        });
        let sizes = std::array::from_fn(|i| {
            counts[i].wrapping_add(i32::from(maximum[i].rem_euclid(self.cell_size[i]) != 0))
        });
        let coarse = DensityVolume::new(
            sizes,
            std::array::from_fn(|i| first[i].wrapping_mul(self.cell_size[i])),
            self.cell_size,
        )?;
        context.with_buffer(coarse.size(), |context, samples| {
            self.input.sample_volume(context, samples, coarse)?;
            if self.fill_whole_cells(output, volume, coarse, samples) {
                return Ok(());
            }
            let [size_x, size_y, size_z] = coarse.sizes();
            let [count_x, count_y, count_z] = counts.map(|count| count.max(0) as usize);
            // Corner samples beyond the coarse grid clamp to its last layer.
            let next = |index: usize, size: usize| (index + 1).min(size - 1);
            for z in 0..count_z {
                let zs = [z, next(z, size_z)];
                for x in 0..count_x {
                    let xs = [x, next(x, size_x)];
                    // Corner bit 0 selects X, bit 2 Z (bit 1, Y, is added per cell).
                    let columns: [usize; 4] =
                        std::array::from_fn(|i| (xs[i & 1] + zs[i >> 1] * size_x) * size_y);
                    for y in 0..count_y {
                        let ys = [y, next(y, size_y)];
                        let corners = std::array::from_fn(|corner| {
                            samples[columns[(corner & 1) | ((corner >> 1) & 2)]
                                + ys[(corner >> 1) & 1]]
                        });
                        self.fill_cell(output, volume, coarse, [x, y, z], corners)?;
                    }
                }
            }
            Ok(())
        })
    }

    /// Interpolates a volume made of whole cells (aligned to them, every size a
    /// multiple of the cell, like a chunk's noise volume; only the top cell may be
    /// cut short), or returns false and leaves it to `fill_cell`. Each value comes
    /// from the same operations on the same inputs as there, in another order: the
    /// four columns of one Z row of a cell run side by side, as lanes LLVM
    /// vectorizes, from the bottom of the volume to the top, each adding its own
    /// increment once per Y. The X lerps at a level of corners are the top of one
    /// cell and the bottom of the next (the same lerps of the same samples), so they
    /// are computed once.
    fn fill_whole_cells(
        &self,
        output: &mut [f32],
        volume: DensityVolume,
        coarse: DensityVolume,
        samples: &[f32],
    ) -> bool {
        let sizes = volume.sizes();
        let cell = self.cell_size.map(|size| size as usize);
        // Every cell has its own level of corners above it: none is clamped.
        let whole = (0..3).all(|i| {
            volume.minimum()[i].rem_euclid(self.cell_size[i]) == 0
                && (i == 1 || sizes[i].is_multiple_of(cell[i]))
                && coarse.sizes()[i] == sizes[i].div_ceil(cell[i]) + 1
        });
        // Lanes run four Xs of one cell; each column stores four Ys at a time.
        if !whole || cell[0] % 4 != 0 || cell[1] % 4 != 0 || samples.len() < coarse.size() {
            return false;
        }
        let Some(output) = output.get_mut(..volume.size()) else {
            return false;
        };
        let [size_x, size_y, size_z] = sizes;
        let [coarse_x, levels, _] = coarse.sizes();
        let column = |x: usize, z: usize| &samples[(x + z * coarse_x) * levels..][..levels];
        for z in 0..size_z {
            let alpha_z = (z % cell[2]) as f32 * self.inverse[2];
            for x in (0..size_x).step_by(4) {
                let alpha_x: [f32; 4] =
                    std::array::from_fn(|lane| (x % cell[0] + lane) as f32 * self.inverse[0]);
                let (cell_x, cell_z) = (x / cell[0], z / cell[2]);
                let (near_0, near_1) = (column(cell_x, cell_z), column(cell_x, cell_z + 1));
                let (far_0, far_1) = (column(cell_x + 1, cell_z), column(cell_x + 1, cell_z + 1));
                // `fill_cell`'s Z lerps at one level of corners, then its `low`.
                let level = |y: usize| -> [f32; 4] {
                    let near = lerp(alpha_z, near_0[y], near_1[y]);
                    let far = lerp(alpha_z, far_0[y], far_1[y]);
                    std::array::from_fn(|lane| lerp(alpha_x[lane], near, far))
                };
                let block = &mut output[(x + z * size_x) * size_y..][..4 * size_y];
                let (a, rest) = block.split_at_mut(size_y);
                let (b, rest) = rest.split_at_mut(size_y);
                let (c, d) = rest.split_at_mut(size_y);
                let cells = a.chunks_exact_mut(cell[1]).zip(b.chunks_exact_mut(cell[1]));
                let cells = cells.zip(c.chunks_exact_mut(cell[1]).zip(d.chunks_exact_mut(cell[1])));
                let mut bottom = level(0);
                let mut cell_y = 0;
                for ((a, b), (c, d)) in cells {
                    let top = level(cell_y + 1);
                    let increment: [f32; 4] =
                        std::array::from_fn(|lane| (top[lane] - bottom[lane]) * self.inverse[1]);
                    // `low + increment * start[1]`, the cell starting at its Y 0.
                    let mut value: [f32; 4] =
                        std::array::from_fn(|lane| bottom[lane] + increment[lane] * 0.0);
                    let quads = a
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(b.as_chunks_mut::<4>().0);
                    let quads = quads.zip(
                        c.as_chunks_mut::<4>()
                            .0
                            .iter_mut()
                            .zip(d.as_chunks_mut::<4>().0),
                    );
                    for ((a, b), (c, d)) in quads {
                        let mut rows = [[0.0; 4]; 4];
                        for row in &mut rows {
                            *row = value;
                            value = std::array::from_fn(|lane| value[lane] + increment[lane]);
                        }
                        for (lane, quad) in [a, b, c, d].into_iter().enumerate() {
                            quad.copy_from_slice(&[
                                rows[0][lane],
                                rows[1][lane],
                                rows[2][lane],
                                rows[3][lane],
                            ]);
                        }
                    }
                    bottom = top;
                    cell_y += 1;
                }
                // The top cell the volume cuts short, if any, one Y at a time.
                let full = cell_y * cell[1];
                if full < size_y {
                    let top = level(cell_y + 1);
                    let increment: [f32; 4] =
                        std::array::from_fn(|lane| (top[lane] - bottom[lane]) * self.inverse[1]);
                    let mut value: [f32; 4] =
                        std::array::from_fn(|lane| bottom[lane] + increment[lane] * 0.0);
                    for y in full..size_y {
                        for (lane, column) in
                            [&mut *a, &mut *b, &mut *c, &mut *d].into_iter().enumerate()
                        {
                            column[y] = value[lane];
                        }
                        value = std::array::from_fn(|lane| value[lane] + increment[lane]);
                    }
                }
            }
        }
        true
    }

    fn fill_cell(
        &self,
        output: &mut [f32],
        volume: DensityVolume,
        coarse: DensityVolume,
        cell: [usize; 3],
        corners: [f32; 8],
    ) -> Result<(), SampleError> {
        let offset: [i32; 3] = std::array::from_fn(|i| {
            coarse.minimum()[i]
                .wrapping_add((cell[i] as i32).wrapping_mul(self.cell_size[i]))
                .wrapping_sub(volume.minimum()[i])
        });
        let start = offset.map(|offset| offset.wrapping_neg().max(0));
        let end: [i32; 3] = std::array::from_fn(|i| {
            self.cell_size[i]
                .min((volume.sizes()[i] as i32).wrapping_sub(offset[i]))
                .wrapping_sub(1)
        });
        if end[1] < start[1] {
            return Ok(());
        }
        let length = (end[1] - start[1] + 1) as usize;
        let output_len = output.len();
        for z in start[2]..=end[2] {
            let alpha_z = z as f32 * self.inverse[2];
            let z00 = lerp(alpha_z, corners[0], corners[4]);
            let z01 = lerp(alpha_z, corners[2], corners[6]);
            let z10 = lerp(alpha_z, corners[1], corners[5]);
            let z11 = lerp(alpha_z, corners[3], corners[7]);
            let column = |x: i32| {
                let alpha_x = x as f32 * self.inverse[0];
                let low = lerp(alpha_x, z00, z10);
                let high = lerp(alpha_x, z01, z11);
                let increment = (high - low) * self.inverse[1];
                let value = low + increment * start[1] as f32;
                let index = volume
                    .index([
                        offset[0].wrapping_add(x) as usize,
                        offset[1].wrapping_add(start[1]) as usize,
                        offset[2].wrapping_add(z) as usize,
                    ])
                    .filter(|index| {
                        index
                            .checked_add(length)
                            .is_some_and(|end| end <= output_len)
                    });
                (index, value, increment)
            };
            // A row four columns wide (a whole cell) runs its columns' independent
            // repeated-addition chains side by side, so their additions overlap.
            if end[0] - start[0] == 3 {
                let columns: [_; 4] = std::array::from_fn(|i| column(start[0] + i as i32));
                if let Some(rows) = four_columns(output, columns.map(|(index, ..)| index), length) {
                    let mut values = columns.map(|(_, value, _)| value);
                    let increments = columns.map(|(.., increment)| increment);
                    let [a, b, c, d] = rows;
                    for y in 0..length {
                        [a[y], b[y], c[y], d[y]] = values;
                        for (value, increment) in values.iter_mut().zip(increments) {
                            *value += increment;
                        }
                    }
                    continue;
                }
            }
            for x in start[0]..=end[0] {
                let (index, mut value, increment) = column(x);
                let index = index.ok_or(DensityError::VolumeTooLarge)?;
                for output in &mut output[index..index + length] {
                    *output = value;
                    // Repeated addition is observable: multiplication by the
                    // current Y offset does not have the same rounding.
                    value += increment;
                }
            }
        }
        Ok(())
    }
}

/// Four disjoint `length`-long column slices of `output` at ascending `starts`, if
/// every start was valid and the columns do not overlap.
fn four_columns(
    output: &mut [f32],
    starts: [Option<usize>; 4],
    length: usize,
) -> Option<[&mut [f32]; 4]> {
    let [a, b, c, d] = starts;
    let [a, b, c, d] = [a?, b?, c?, d?];
    if b.checked_sub(a)? < length || c.checked_sub(b)? < length || d.checked_sub(c)? < length {
        return None;
    }
    let (first, rest) = output.get_mut(a..)?.split_at_mut(b - a);
    let (second, rest) = rest.split_at_mut(c - b);
    let (third, fourth) = rest.split_at_mut(d - c);
    Some([
        &mut first[..length],
        &mut second[..length],
        &mut third[..length],
        fourth.get_mut(..length)?,
    ])
}

fn lerp(alpha: f32, first: f32, second: f32) -> f32 {
    first + alpha * (second - first)
}
impl DensitySampler for InterpolatedSampler {
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        let remainder: [i32; 3] =
            std::array::from_fn(|i| position[i].rem_euclid(self.cell_size[i]));
        if remainder == [0; 3] {
            return self.input.sample_value(context, position);
        }
        let volume = DensityVolume::new(
            [2; 3],
            std::array::from_fn(|i| position[i].wrapping_sub(remainder[i])),
            self.cell_size,
        )?;
        context.with_buffer(8, |context, corners| {
            self.input.sample_volume(context, corners, volume)?;
            let alpha: [f32; 3] =
                std::array::from_fn(|i| remainder[i] as f32 / self.cell_size[i] as f32);
            // Samples use Y-innermost storage; interpolation blends X, Y, Z.
            let xy0 = lerp(
                alpha[1],
                lerp(alpha[0], corners[0], corners[2]),
                lerp(alpha[0], corners[1], corners[3]),
            );
            let xy1 = lerp(
                alpha[1],
                lerp(alpha[0], corners[4], corners[6]),
                lerp(alpha[0], corners[5], corners[7]),
            );
            Ok(lerp(alpha[2], xy0, xy1))
        })
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        output: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_buffer(output, volume)?;
        if context.sparse {
            if (0..3).all(|i| {
                volume.steps()[i] % self.cell_size[i] == 0
                    && volume.minimum()[i].rem_euclid(self.cell_size[i]) == 0
            }) {
                return self.input.sample_volume(context, output, volume);
            }
            return visit(volume, |index, position| {
                output[index] = self.sample_value(context, position)?;
                Ok(())
            });
        }
        if (0..3).all(|i| {
            (volume.steps()[i] == self.cell_size[i] || volume.sizes()[i] == 1)
                && volume.minimum()[i].rem_euclid(self.cell_size[i]) == 0
        }) {
            return self.input.sample_volume(context, output, volume);
        }
        if volume.steps() == [1; 3] {
            return self.sample_block_volume(context, output, volume);
        }
        // The reference evaluates the complete block-resolution bounding volume
        // before selecting strided samples, including trailing unsampled blocks.
        let dense = DensityVolume::new(
            std::array::from_fn(|i| (volume.sizes()[i] as i32).wrapping_mul(volume.steps()[i])),
            volume.minimum(),
            [1; 3],
        )?;
        context.with_buffer(dense.size(), |context, samples| {
            self.sample_block_volume(context, samples, dense)?;
            let [sx, sy, sz] = volume.sizes();
            for z in 0..sz {
                for x in 0..sx {
                    for y in 0..sy {
                        let source = dense.index(std::array::from_fn(|i| {
                            [x, y, z][i] * volume.steps()[i] as usize
                        }));
                        output[(x + z * sx) * sy + y] = samples[source.unwrap()];
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An input with a different value at every block: ordinary values of very
    /// different magnitudes, and now and then a NaN, an infinity, a signed zero or
    /// a subnormal, so a changed operation or operand order would show.
    #[derive(Debug)]
    struct Lattice;
    impl Lattice {
        fn value(position: [i32; 3]) -> f32 {
            let mut hash = (position[0] as u32).wrapping_mul(0x9E37_79B1)
                ^ (position[1] as u32).wrapping_mul(0x85EB_CA77)
                ^ (position[2] as u32).wrapping_mul(0xC2B2_AE3D);
            hash ^= hash >> 15;
            hash = hash.wrapping_mul(0x2C1B_3C6D);
            hash ^= hash >> 13;
            const SPECIAL: [f32; 8] = [
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                0.0,
                -0.0,
                1e-40,
                -3e38,
                3e38,
            ];
            const SCALE: [f32; 4] = [1.0, 1e-3, 1e30, 3e-39];
            if hash.is_multiple_of(23) {
                SPECIAL[(hash >> 5) as usize % 8]
            } else {
                ((hash >> 8) as f32 / (1 << 24) as f32 - 0.5) * SCALE[(hash >> 3) as usize % 4]
            }
        }
    }
    impl DensitySampler for Lattice {
        fn sample_value(
            &self,
            _: &mut SamplerContext,
            position: [i32; 3],
        ) -> Result<f32, SampleError> {
            Ok(Self::value(position))
        }
    }

    /// Vanilla's `InterpolatedFunction.Sampler.sampleWithBlockStep` and `fillCell`,
    /// transcribed (it carries a cell's top corners over to the next cell up; they
    /// are the same samples read again here).
    fn vanilla(cell_xz: i32, cell_y: i32, volume: DensityVolume) -> Vec<f32> {
        let size = [cell_xz, cell_y, cell_xz];
        let (xz_inv, y_inv) = (1.0 / cell_xz as f32, 1.0 / cell_y as f32);
        let (min, max) = (volume.minimum(), volume.maximum());
        let first: [i32; 3] = std::array::from_fn(|i| min[i].div_euclid(size[i]));
        let count: [i32; 3] = std::array::from_fn(|i| max[i].div_euclid(size[i]) - first[i] + 1);
        let cells: [i32; 3] =
            std::array::from_fn(|i| count[i] + i32::from(max[i].rem_euclid(size[i]) != 0));
        let corner = |x: i32, y: i32, z: i32| {
            Lattice::value([
                (first[0] + x) * size[0],
                (first[1] + y) * size[1],
                (first[2] + z) * size[2],
            ])
        };
        let lerp = |delta: f32, start: f32, end: f32| start + delta * (end - start);
        let sizes = volume.sizes().map(|size| size as i32);
        let mut output = vec![f32::NAN; volume.size()];
        for cell_z in 0..count[2] {
            let next_z = (cell_z + 1).min(cells[2] - 1);
            for cell_x in 0..count[0] {
                let next_x = (cell_x + 1).min(cells[0] - 1);
                for cell_y in 0..count[1] {
                    let next_y = (cell_y + 1).min(cells[1] - 1);
                    let (v000, v100) = (
                        corner(cell_x, cell_y, cell_z),
                        corner(next_x, cell_y, cell_z),
                    );
                    let (v010, v110) = (
                        corner(cell_x, next_y, cell_z),
                        corner(next_x, next_y, cell_z),
                    );
                    let (v001, v101) = (
                        corner(cell_x, cell_y, next_z),
                        corner(next_x, cell_y, next_z),
                    );
                    let (v011, v111) = (
                        corner(cell_x, next_y, next_z),
                        corner(next_x, next_y, next_z),
                    );
                    let out: [i32; 3] = std::array::from_fn(|i| {
                        (first[i] + [cell_x, cell_y, cell_z][i]) * size[i] - min[i]
                    });
                    let lo = out.map(|out| 0.max(-out));
                    let hi: [i32; 3] = std::array::from_fn(|i| size[i].min(sizes[i] - out[i]) - 1);
                    for z in lo[2]..=hi[2] {
                        let alpha_z = z as f32 * xz_inv;
                        let v00_ = lerp(alpha_z, v000, v001);
                        let v01_ = lerp(alpha_z, v010, v011);
                        let v10_ = lerp(alpha_z, v100, v101);
                        let v11_ = lerp(alpha_z, v110, v111);
                        for x in lo[0]..=hi[0] {
                            let alpha_x = x as f32 * xz_inv;
                            let v_0_ = lerp(alpha_x, v00_, v10_);
                            let v_1_ = lerp(alpha_x, v01_, v11_);
                            let step = (v_1_ - v_0_) * y_inv;
                            let mut value = v_0_ + step * lo[1] as f32;
                            let at = [out[0] + x, out[1] + lo[1], out[2] + z].map(|v| v as usize);
                            let index = volume.index(at).unwrap();
                            let length = (hi[1] - lo[1] + 1) as usize;
                            for slot in &mut output[index..index + length] {
                                *slot = value;
                                value += step;
                            }
                        }
                    }
                }
            }
        }
        output
    }

    fn same(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(a, b)| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()))
    }

    #[test]
    fn whole_cell_volumes_match_vanilla_bit_for_bit() {
        // (minimum, sizes, whether the whole-cell path takes it with 4-wide cells,
        // and with 8-wide ones, if its minimum Y is on a cell boundary).
        let cases = [
            ([-16, -64, 32], [16, 384, 16], true, true),
            ([0, -64, 0], [16, 114, 16], true, true), // the top cell cut short
            ([0, -64, 0], [16, 113, 16], false, false), // the top cell has no corners above
            ([-8, 0, 8], [8, 8, 8], true, true),
            ([4, 0, 0], [12, 16, 4], true, false), // 8-wide cells do not divide it
            ([1, 0, 0], [16, 16, 16], false, false), // not aligned
            ([0, 4, 0], [16, 16, 16], true, true), // aligned in Y to 4-high cells only
        ];
        for (cell_xz, cell_y) in [(4, 8), (8, 4), (4, 4), (8, 8)] {
            let sampler = InterpolatedSampler::new(Arc::new(Lattice), cell_xz, cell_y).unwrap();
            for (minimum, sizes, takes_4, takes_8) in cases {
                let volume = DensityVolume::new(sizes, minimum, [1; 3]).unwrap();
                let expected = vanilla(cell_xz, cell_y, volume);
                let mut context = SamplerContext::default();
                let mut output = vec![1.5; volume.size()];
                sampler
                    .sample_volume(&mut context, &mut output, volume)
                    .unwrap();
                assert!(
                    same(&output, &expected),
                    "{cell_xz}x{cell_y} {minimum:?} {sizes:?}"
                );

                // The whole-cell path itself, with every position written.
                let first: [i32; 3] =
                    std::array::from_fn(|i| minimum[i].div_euclid(sampler.cell_size[i]));
                let levels: [i32; 3] = std::array::from_fn(|i| {
                    (volume.maximum()[i].div_euclid(sampler.cell_size[i]) - first[i] + 1)
                        + i32::from(volume.maximum()[i].rem_euclid(sampler.cell_size[i]) != 0)
                });
                let coarse = DensityVolume::new(
                    levels,
                    std::array::from_fn(|i| first[i] * sampler.cell_size[i]),
                    sampler.cell_size,
                )
                .unwrap();
                let mut samples = vec![0.0; coarse.size()];
                Lattice
                    .sample_volume(&mut context, &mut samples, coarse)
                    .unwrap();
                let mut output = vec![2.5; volume.size()];
                let taken = sampler.fill_whole_cells(&mut output, volume, coarse, &samples);
                let expected_taken =
                    if cell_xz == 4 { takes_4 } else { takes_8 } && minimum[1] % cell_y == 0;
                assert_eq!(
                    taken, expected_taken,
                    "{cell_xz}x{cell_y} {minimum:?} {sizes:?}"
                );
                if taken {
                    assert!(
                        same(&output, &expected),
                        "{cell_xz}x{cell_y} {minimum:?} {sizes:?}"
                    );
                }
            }
        }
    }
}
