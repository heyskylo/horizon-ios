//! Polytone's `r.noise(x, y)`: 1.18-1.21's `PerlinSimplexNoise` over simplex noises, which 26.3 no
//! longer has. Measured on the real mod (Polytone 26.3-6.8.6) with a pack whose grass colormap encodes
//! the noise: the built-in noise is seed 0 with octaves `{1}`, read with the noise offsets
//! (`getValue(x, y, true)`), and a `polytone/noises` definition with seed 0 or 1 and octaves `{0}`,
//! `{1}` or `{0, 1}` matches this construction too.

use horizon_core::noise::SimplexNoise;
use horizon_core::random::{LegacyRandom, RandomSource};

/// `PerlinSimplexNoise(random, octaves)`: one simplex noise per octave, the highest octave first.
pub(crate) struct PerlinSimplex {
    levels: Vec<Option<SimplexNoise>>,
    input_factor: f64,
    value_factor: f64,
}

impl PerlinSimplex {
    /// The construction of `PerlinSimplexNoise(new LegacyRandomSource(seed), octaves)`: octave 0 takes
    /// the first noise of `seed`'s random; the positive octaves come from a random seeded by that
    /// noise's value at its own offsets, the negative ones from `seed`'s random after it.
    pub(crate) fn new(seed: i64, octaves: &[i32]) -> Self {
        let mut random = LegacyRandom::new(seed);
        let lowest = octaves.iter().copied().min().unwrap_or(0);
        let highest = octaves.iter().copied().max().unwrap_or(0);
        let count = (highest - lowest + 1) as usize;
        let first = SimplexNoise::new(&mut random);
        let top = highest;
        let wanted = |index: i32| octaves.contains(&(top - index));
        let mut levels: Vec<Option<SimplexNoise>> = (0..count).map(|_| None).collect();
        for index in top + 1..count as i32 {
            if index >= 0 && wanted(index) {
                levels[index as usize] = Some(SimplexNoise::new(&mut random));
            } else {
                random.consume_count(262);
            }
        }
        if highest > 0 {
            // `(long)(getValue(xo, yo, zo) * 9.223372E18F)`: the noise at its own offsets, which
            // 26.3's get3 reads at (0, 0, 0). The float constant is 2^63.
            let value = first.get3_f64(0.0, 0.0, 0.0) * 9.223_372e18_f32 as f64;
            let mut upper = LegacyRandom::new(value as i64);
            for index in (0..top).rev() {
                if (index as usize) < count && wanted(index) {
                    levels[index as usize] = Some(SimplexNoise::new(&mut upper));
                } else {
                    upper.consume_count(262);
                }
            }
        }
        if (0..count as i32).contains(&top) && octaves.contains(&0) {
            levels[top as usize] = Some(first);
        }
        Self {
            levels,
            input_factor: 2f64.powi(highest),
            value_factor: 1.0 / (2f64.powi(count as i32) - 1.0),
        }
    }

    /// `getValue(x, y, true)`: each octave read with its offsets, which 26.3's simplex adds itself.
    pub(crate) fn get(&self, x: f64, y: f64) -> f64 {
        let (mut input, mut value, mut sum) = (self.input_factor, self.value_factor, 0.0);
        for level in &self.levels {
            if let Some(noise) = level {
                sum += noise.get2_f64(x * input, y * input) * value;
            }
            input /= 2.0;
            value *= 2.0;
        }
        sum
    }
}

/// The built-in `r.noise(x, y)`: seed 0, octaves `{1}`.
pub(crate) fn builtin() -> &'static PerlinSimplex {
    static NOISE: std::sync::OnceLock<PerlinSimplex> = std::sync::OnceLock::new();
    NOISE.get_or_init(|| PerlinSimplex::new(0, &[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `r.noise(x/8, z/8)` read on the real mod (Polytone 26.3-6.8.6): a grass colormap whose pixel
    /// encodes the value, decoded to about 0.01 (polytone-work/rnoise in the measurement notes).
    #[test]
    fn the_builtin_noise_reads_what_the_mod_draws() {
        for (x, z, mod_value) in [
            (0, 10, -0.28373),
            (3, 10, -0.14683),
            (10, 0, 0.31833),
            (24, 14, -0.55941),
            (40, 25, -0.48358),
            (7, 3, -0.68202),
        ] {
            let value = builtin().get(f64::from(x) / 8.0, f64::from(z) / 8.0);
            assert!(
                (value - mod_value).abs() < 0.02,
                "({x}, {z}): {value} against {mod_value}"
            );
        }
    }

    /// The octaves add as `PerlinSimplexNoise`'s do: `{0, 1}` is a third of `{1}` and two thirds of
    /// `{0}` (measured: 0.067 = 0.33 x -0.284 + 0.67 x 0.247 at (0, 10) / 8).
    #[test]
    fn two_octaves_weigh_a_third_and_two_thirds() {
        let both = PerlinSimplex::new(0, &[0, 1]);
        let (low, high) = (PerlinSimplex::new(0, &[0]), PerlinSimplex::new(0, &[1]));
        for (x, y) in [(0.0, 1.25), (2.5, -3.0), (17.1, 4.4)] {
            let mixed = high.get(x, y) / 3.0 + low.get(x, y) * 2.0 / 3.0;
            assert!((both.get(x, y) - mixed).abs() < 1e-12);
        }
        assert!((both.get(0.0, 1.25) - 0.067).abs() < 0.02);
    }
}
