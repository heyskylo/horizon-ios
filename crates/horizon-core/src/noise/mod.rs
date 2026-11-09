//! Base noise samplers for the pinned 26.3 target.
//!
//! Coordinates use double precision; Perlin interpolation and its derivatives
//! use single precision. Keep operation order: algebraic rearrangement changes
//! world generation. Complete density graphs and terrain generation remain absent.
mod normal;
mod perlin;
mod simplex;
mod stack;
mod volume;

pub use normal::{
    NoiseBuildError, NormalNoise, NormalNoiseBuilder, Normalization, OctaveInfo, create_legacy_fbm,
};
pub use perlin::{PerlinNoise, SmearedPerlinNoise};
pub use simplex::SimplexNoise;
pub use stack::{Noise, NoiseLayer, NoiseStack, NoiseStackBuilder};
pub use volume::{DensityError, DensityVolume};

use crate::random::RandomSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NanRange;
impl std::fmt::Display for NanRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("noise interval bounds cannot include NaN")
    }
}
impl std::error::Error for NanRange {}

/// The immutable offsets and permutation table shared by gradient samplers.
#[derive(Clone, Debug)]
pub struct GradientTable {
    offsets: [f64; 3],
    permutations: [u8; 256],
}

impl GradientTable {
    pub fn new(random: &mut impl RandomSource, offset_scale: f64) -> Self {
        // Zero scale still consumes all three draws.
        let offsets = std::array::from_fn(|_| random.next_double() * offset_scale);
        let mut permutations = std::array::from_fn(|i| i as u8);
        for i in 0..256 {
            let other = i + random.next_int_bound((256 - i) as i32).unwrap() as usize;
            permutations.swap(i, other);
        }
        Self {
            offsets,
            permutations,
        }
    }

    pub fn offsets(&self) -> &[f64; 3] {
        &self.offsets
    }
    pub fn permutations(&self) -> &[u8; 256] {
        &self.permutations
    }

    fn permute(&self, index: i32) -> i32 {
        i32::from(self.permutations[(index & 255) as usize])
    }
}

// Perlin's 16-direction table includes four repeated directions. Simplex uses
// the first twelve directions, selecting them with remainder instead of masking.
const GRADIENTS: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

/// [`GRADIENTS`] as floats, so a dot product needs no conversions: every
/// component is -1, 0 or 1, which a float holds exactly.
const PERLIN_GRADIENTS: [[f32; 3]; 16] = {
    let mut out = [[0.0; 3]; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = [
            GRADIENTS[i][0] as f32,
            GRADIENTS[i][1] as f32,
            GRADIENTS[i][2] as f32,
        ];
        i += 1;
    }
    out
};

fn dot(gradient: [f32; 3], point: [f32; 3]) -> f32 {
    gradient[0] * point[0] + gradient[1] * point[1] + gradient[2] * point[2]
}

/// Vanilla's periodic coordinate reduction with the normal Far Lands flag off.
/// The fast-path boundary is the double immediately below 2^24.
pub fn wrap(value: f64) -> f64 {
    let half = f64::from_bits(16_777_216.0_f64.to_bits() - 1);
    if value >= -half && value < half {
        value
    } else {
        value - (value / 33_554_432.0 + 0.5).floor() * 33_554_432.0
    }
}

fn smoothstep(value: f32) -> f32 {
    value * value * value * (value * (value * 6.0 - 15.0) + 10.0)
}
fn smoothstep_derivative(value: f32) -> f32 {
    30.0 * value * value * (value - 1.0) * (value - 1.0)
}
fn lerp(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * (end - start)
}
fn lerp2(a: f32, b: f32, values: [f32; 4]) -> f32 {
    lerp(
        b,
        lerp(a, values[0], values[1]),
        lerp(a, values[2], values[3]),
    )
}
fn lerp3(fade: [f32; 3], values: [f32; 8]) -> f32 {
    lerp(
        fade[2],
        lerp2(fade[0], fade[1], values[..4].try_into().unwrap()),
        lerp2(fade[0], fade[1], values[4..].try_into().unwrap()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{geometry::Axis, random::LegacyRandom};

    #[test]
    fn vanilla_seed_zero_samples_and_derivatives() {
        // Values captured by the original adapter from the pinned server JAR.
        let mut random = LegacyRandom::new(0);
        let noise = PerlinNoise::new(&mut random);
        assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
        let mut derivative = [0.0; 3];
        assert_eq!(
            noise
                .noise_with_derivative(0.0, 0.0, 0.0, &mut derivative)
                .to_bits(),
            3_183_733_582
        );
        assert_eq!(
            derivative.map(f32::to_bits),
            [3_189_117_465, 3_214_556_526, 1_017_326_887]
        );
        let mut random = LegacyRandom::new(0);
        let simplex = SimplexNoise::with_skipped_offsets(&mut random, true);
        assert_eq!(simplex.table().offsets(), &[0.0; 3]);
        assert_eq!(simplex.table().permutations(), noise.table().permutations());
        assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
        assert_eq!(simplex.get2(0.25, 0.75).to_bits(), 3_146_180_247);
        assert_eq!(simplex.get3(0.25, 0.5, 0.75).to_bits(), 1_062_011_824);
        assert!(simplex.get3(f64::NAN, 0.0, 0.0).is_nan());
        assert_eq!(
            SmearedPerlinNoise::new(&mut random, f64::NAN).range(),
            Err(NanRange)
        );
    }

    #[test]
    fn bulk_accumulation_preserves_tail_and_rejects_short_buffers_before_writes() {
        let noise = PerlinNoise::new(&mut LegacyRandom::new(0));
        let volume = DensityVolume::new([1; 3], [0; 3], [1; 3]).unwrap();
        let mut random = LegacyRandom::new(0);
        let mut values =
            std::array::from_fn::<_, 5, _>(|_| f32::from_bits(random.next_int() as u32));
        let tail = values[1..].iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        noise
            .add_to_volume(&mut values, volume, 1.0, 1.0, 1.0)
            .unwrap();
        assert_eq!(values[0].to_bits(), 3_184_062_705);
        assert_eq!(
            values[1..].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            tail
        );
        let before = values.map(f32::to_bits);
        let too_large = DensityVolume::new([2; 3], [0; 3], [1; 3]).unwrap();
        assert_eq!(
            noise.add_to_volume(&mut values, too_large, 1.0, 1.0, 1.0),
            Err(DensityError::BufferTooShort)
        );
        assert_eq!(values.map(f32::to_bits), before);
    }

    #[test]
    fn density_dimensions_are_bounded_and_coordinates_wrap() {
        assert_eq!(
            DensityVolume::new([0, 1, 1], [0; 3], [1; 3]),
            Err(DensityError::NonpositiveDimensions)
        );
        assert_eq!(
            DensityVolume::new([1; 3], [0; 3], [1, 0, 1]),
            Err(DensityError::NonpositiveSteps)
        );
        assert_eq!(
            DensityVolume::new([i32::MAX; 3], [0; 3], [1; 3]),
            Err(DensityError::VolumeTooLarge)
        );
        let volume = DensityVolume::new([2, 3, 4], [i32::MAX, 0, -1], [2, 4, 8]).unwrap();
        assert_eq!(volume.block(Axis::X, 1), Some(i32::MIN + 1));
        assert_eq!(volume.block(Axis::X, 2), None);
        assert_eq!(volume.index([1, 2, 3]), Some(23));
        assert_eq!(volume.index([1, 3, 3]), None);
    }

    #[test]
    fn normal_noise_reference_values_and_legacy_failure_draws() {
        let mut builder = NormalNoise::builder();
        builder.set_base_octave(-3);
        assert_eq!(
            builder.set_amplitude_modifier(-1, 0.0).unwrap_err(),
            NoiseBuildError::ModifierIndex
        );
        builder.set_octave_count(3).unwrap();
        builder.set_amplitude_modifier(1, 0.0).unwrap();
        assert_eq!(
            builder.set_octave_count(4).unwrap_err(),
            NoiseBuildError::CountAfterModifiers
        );
        let definition = builder.build().unwrap();
        assert_eq!(
            definition.normalization_factor().to_bits(),
            4_607_442_513_526_237_848
        );
        assert_eq!(
            definition
                .octaves()
                .iter()
                .map(|o| o.octave_index)
                .collect::<Vec<_>>(),
            [-3, -1]
        );
        let mut random = LegacyRandom::new(0);
        let modern = definition.create(&mut random).unwrap();
        assert_eq!(modern.get3(0.0, 0.0, 0.0).to_bits(), 1_042_650_710);
        assert_eq!(random.next_long(), -6_688_467_811_848_818_630);
        let mut random = LegacyRandom::new(0);
        let legacy = definition
            .create_for_legacy_nether_biome(&mut random)
            .unwrap();
        assert_eq!(legacy.get3(0.0, 0.0, 0.0).to_bits(), 3_199_632_936);
        assert_eq!(random.next_long(), 4_966_550_093_728_345_520);
        let mut empty = NormalNoise::builder();
        empty.set_base_octave(2).set_octave_count(0).unwrap();
        let mut random = LegacyRandom::new(0);
        assert_eq!(
            empty
                .build()
                .unwrap()
                .create_for_legacy_nether_biome(&mut random)
                .unwrap_err(),
            NoiseBuildError::PositiveLegacyOctaves
        );
        assert_eq!(random.next_long(), -6_819_959_017_335_327_829);
        let mut random = LegacyRandom::new(0);
        assert_eq!(
            create_legacy_fbm(&mut random, i32::MIN, &[1.0]).unwrap_err(),
            NoiseBuildError::LegacyWorkLimit
        );
        assert_eq!(random.next_long(), LegacyRandom::new(0).next_long());
    }

    #[test]
    fn stack_snapshots_share_layers_and_preserve_zero_amplitude_semantics() {
        use std::sync::Arc;
        let sampler: Arc<dyn Noise> = Arc::new(PerlinNoise::new(&mut LegacyRandom::new(0)));
        let mut builder = NoiseStack::builder();
        builder.add(Arc::clone(&sampler), 1.0, 0.0);
        let zero = builder.build().unwrap();
        builder.add(Arc::clone(&sampler), 2.0, 1.0);
        assert_eq!(zero.layers().len(), 1);
        assert!(Arc::ptr_eq(zero.get_layer(0).unwrap(), &sampler));
        assert!(zero.get_layer(1).is_none());
        let mut flattened = NoiseStack::builder();
        flattened.add_stack(&zero, 1.0, f32::INFINITY);
        assert_eq!(flattened.build().unwrap_err(), NanRange);
        let nested = NoiseStack::builder()
            .add(Arc::new(zero), 1.0, f32::INFINITY)
            .build()
            .unwrap();
        assert_eq!(nested.range().unwrap(), [0.0; 2]);
        assert!(nested.get3(1.0, 2.0, 3.0).is_nan());
    }
}
