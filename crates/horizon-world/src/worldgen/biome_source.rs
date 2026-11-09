//! Fixed, checkerboard and End biome-source queries. Registry codecs are separate.
mod codec;
mod runtime;
pub use codec::{BiomeSet, BiomeSourceCatalog, BiomeSourceDefinition};
use horizon_core::density::{DensitySampler, SampleError, SamplerContext};
pub use runtime::{BiomeSourceError, BiomeSourceResolver, ResolvedBiomeSource};

#[derive(Clone, Debug)]
pub struct FixedBiomeSource<T> {
    pub biome: T,
}
impl<T> FixedBiomeSource<T> {
    pub fn noise_biome(&self, _quart: [i32; 3]) -> &T {
        &self.biome
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckerboardError {
    TooManyBiomes,
    EmptyBiomes,
}
impl std::fmt::Display for CheckerboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CheckerboardError {}
#[derive(Clone, Debug)]
pub struct CheckerboardBiomeSource<T> {
    biomes: Vec<T>,
    scale: i32,
}
impl<T> CheckerboardBiomeSource<T> {
    /// Runtime scale is unrestricted, including Java's masked shift distances.
    /// The vanilla JSON codec separately limits scale to 0..=62.
    pub fn new(biomes: Vec<T>, scale: i32) -> Result<Self, CheckerboardError> {
        if biomes.len() > 8192 {
            return Err(CheckerboardError::TooManyBiomes);
        }
        Ok(Self { biomes, scale })
    }
    pub fn noise_biome(&self, quart: [i32; 3]) -> Result<&T, CheckerboardError> {
        if self.biomes.is_empty() {
            return Err(CheckerboardError::EmptyBiomes);
        }
        let shift = self.scale.wrapping_add(2) as u32;
        let cell = quart[0]
            .wrapping_shr(shift)
            .wrapping_add(quart[2].wrapping_shr(shift));
        Ok(&self.biomes[cell.rem_euclid(self.biomes.len() as i32) as usize])
    }
}
#[derive(Clone, Debug)]
pub struct EndBiomeSource<T> {
    pub center: T,
    pub highlands: T,
    pub midlands: T,
    pub islands: T,
    pub barrens: T,
}
impl<T> EndBiomeSource<T> {
    /// The central 64-chunk circle bypasses erosion. Outside it, sample erosion
    /// at the containing chunk's center with the original wrapped block Y.
    pub fn noise_biome(
        &self,
        quart: [i32; 3],
        erosion: &dyn DensitySampler,
        context: &mut SamplerContext,
    ) -> Result<&T, SampleError> {
        let block = quart.map(|v| v.wrapping_shl(2));
        let x = block[0] >> 4;
        let z = block[2] >> 4;
        if i64::from(x) * i64::from(x) + i64::from(z) * i64::from(z) <= 4096 {
            return Ok(&self.center);
        }
        let position = [
            x.wrapping_mul(2).wrapping_add(1).wrapping_mul(8),
            block[1],
            z.wrapping_mul(2).wrapping_add(1).wrapping_mul(8),
        ];
        let value = erosion.sample_value(context, position)?;
        Ok(if value > 0.25 {
            &self.highlands
        } else if value >= -0.0625 {
            &self.midlands
        } else if value < -0.21875 {
            &self.islands
        } else {
            &self.barrens
        })
    }
}
