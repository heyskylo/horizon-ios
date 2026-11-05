use super::*;

#[derive(Clone, Debug)]
pub struct RangeChoiceSampler {
    pub input: Arc<dyn DensitySampler>,
    pub min_inclusive: f32,
    pub max_exclusive: f32,
    pub when_in_range: Arc<dyn DensitySampler>,
    pub when_out_of_range: Arc<dyn DensitySampler>,
}
impl RangeChoiceSampler {
    fn contains(&self, value: f32) -> bool {
        value >= self.min_inclusive && value < self.max_exclusive
    }
}
impl DensitySampler for RangeChoiceSampler {
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        if self.contains(self.input.sample_value(context, position)?) {
            self.when_in_range.sample_value(context, position)
        } else {
            self.when_out_of_range.sample_value(context, position)
        }
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_exact_buffer(buffer, volume)?;
        // Vanilla evaluates the in-range branch before the selector in bulk.
        self.when_in_range.sample_volume(context, buffer, volume)?;
        context.with_buffer(volume.size(), |context, input| {
            self.input.sample_volume(context, input, volume)?;
            context.with_buffer(volume.size(), |context, outside| {
                self.when_out_of_range
                    .sample_volume(context, outside, volume)?;
                // A select rather than a conditional store, so the loop vectorizes.
                for ((value, &input), &outside) in buffer.iter_mut().zip(&*input).zip(&*outside) {
                    *value = if self.contains(input) {
                        *value
                    } else {
                        outside
                    };
                }
                Ok(())
            })
        })
    }
}

/// An explicitly compiled value. Distinguishing literal constants here avoids
/// scratch allocation in the constant-endpoint interpolation specializations.
#[derive(Clone, Debug)]
pub enum DensityValue {
    Constant(f32),
    Sampler(Arc<dyn DensitySampler>),
}
impl DensityValue {
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Sampler(sampler) => sampler.sample_value(context, position),
        }
    }
    fn with_volume<T>(
        &self,
        context: &mut SamplerContext,
        volume: DensityVolume,
        use_values: impl FnOnce(&mut SamplerContext, Values<'_>) -> Result<T, SampleError>,
    ) -> Result<T, SampleError> {
        match self {
            Self::Constant(value) => use_values(context, Values::Constant(*value)),
            Self::Sampler(sampler) => context.with_buffer(volume.size(), |context, buffer| {
                sampler.sample_volume(context, buffer, volume)?;
                use_values(context, Values::Buffer(buffer))
            }),
        }
    }
}
#[derive(Clone, Copy)]
enum Values<'a> {
    Constant(f32),
    Buffer(&'a [f32]),
}
impl Values<'_> {
    fn get(self, index: usize) -> f32 {
        match self {
            Self::Constant(value) => value,
            Self::Buffer(buffer) => buffer[index],
        }
    }
}
#[derive(Clone, Debug)]
pub struct LerpSampler {
    pub alpha: Arc<dyn DensitySampler>,
    pub first: DensityValue,
    pub second: DensityValue,
}
fn interpolate(alpha: f32, first: f32, second: f32) -> f32 {
    if alpha == 0.0 {
        first
    } else if alpha == 1.0 {
        second
    } else {
        first + alpha * (second - first)
    }
}
impl DensitySampler for LerpSampler {
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        let alpha = self.alpha.sample_value(context, position)?;
        if alpha == 0.0 {
            return self.first.sample_value(context, position);
        }
        if alpha == 1.0 {
            return self.second.sample_value(context, position);
        }
        let first = self.first.sample_value(context, position)?;
        let second = self.second.sample_value(context, position)?;
        Ok(interpolate(alpha, first, second))
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_exact_buffer(buffer, volume)?;
        self.alpha.sample_volume(context, buffer, volume)?;
        self.first.with_volume(context, volume, |context, first| {
            self.second.with_volume(context, volume, |_, second| {
                for (index, alpha) in buffer.iter_mut().enumerate() {
                    *alpha = interpolate(*alpha, first.get(index), second.get(index));
                }
                Ok(())
            })
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidIntervals;
impl std::fmt::Display for InvalidIntervals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("compiled interval selection requires one more branch than thresholds, at most 256 branches")
    }
}
impl std::error::Error for InvalidIntervals {}

/// Compiled interval selection. Threshold order is preserved, including NaNs
/// and equal thresholds; sorting and codec validation belong to the compiler.
#[derive(Clone, Debug)]
pub struct IntervalSelectSampler {
    input: Arc<dyn DensitySampler>,
    thresholds: Vec<f32>,
    branches: Vec<Arc<dyn DensitySampler>>,
}
impl IntervalSelectSampler {
    pub fn new(
        input: Arc<dyn DensitySampler>,
        thresholds: Vec<f32>,
        branches: Vec<Arc<dyn DensitySampler>>,
    ) -> Result<Self, InvalidIntervals> {
        if branches.len() > 256 || thresholds.len().checked_add(1) != Some(branches.len()) {
            return Err(InvalidIntervals);
        }
        Ok(Self {
            input,
            thresholds,
            branches,
        })
    }
    fn select(&self, value: f32) -> usize {
        self.thresholds
            .iter()
            .position(|threshold| value < *threshold)
            .unwrap_or(self.branches.len() - 1)
    }
}
impl DensitySampler for IntervalSelectSampler {
    fn sample_value(
        &self,
        context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        let value = self.input.sample_value(context, position)?;
        self.branches[self.select(value)].sample_value(context, position)
    }
    fn sample_volume(
        &self,
        context: &mut SamplerContext,
        buffer: &mut [f32],
        volume: DensityVolume,
    ) -> Result<(), SampleError> {
        check_exact_buffer(buffer, volume)?;
        let total = volume
            .size()
            .checked_mul(self.branches.len())
            .filter(|count| *count <= isize::MAX as usize / size_of::<f32>())
            .ok_or(DensityError::VolumeTooLarge)?;
        self.input.sample_volume(context, buffer, volume)?;
        // Contiguous retained storage replaces a separate allocation per branch.
        // Each child still receives a slice with exactly the logical volume size.
        context.with_buffer(total, |context, values| {
            for (branch, output) in self
                .branches
                .iter()
                .zip(values.chunks_exact_mut(volume.size()))
            {
                branch.sample_volume(context, output, volume)?;
            }
            // Delay every output selection until all child volumes have returned.
            for (index, input) in buffer.iter_mut().enumerate() {
                *input = values[self.select(*input) * volume.size() + index];
            }
            Ok(())
        })
    }
}
