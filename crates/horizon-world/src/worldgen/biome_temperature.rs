//! Biome temperature sampling and a worker-local vanilla temperature cache.
use super::{DecodeError, Object, number_text, optional, required};
use horizon_core::{
    noise::{Noise, NoiseStack, SimplexNoise},
    position::BlockPos,
    random::LegacyRandom,
};
use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    sync::{Arc, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemperatureModifier {
    None,
    Frozen,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}
#[derive(Clone, Copy, Debug)]
pub struct BiomeTemperatureSettings {
    pub base_temperature: f32,
    pub modifier: TemperatureModifier,
    pub has_precipitation: bool,
    pub downfall: f32,
}
struct TemperatureNoises {
    height: SimplexNoise,
    frozen: NoiseStack,
    info: SimplexNoise,
}
fn noises() -> &'static TemperatureNoises {
    static NOISES: OnceLock<TemperatureNoises> = OnceLock::new();
    NOISES.get_or_init(|| {
        let mut random = LegacyRandom::new(3456);
        let mut frozen = NoiseStack::builder();
        for (frequency, amplitude) in [
            (1.0, 1.0f32 / 7.0),
            (0.5, 2.0f32 / 7.0),
            (0.25, 4.0f32 / 7.0),
        ] {
            frozen.add(
                Arc::new(SimplexNoise::with_skipped_offsets(&mut random, true)),
                frequency,
                amplitude,
            );
        }
        TemperatureNoises {
            height: SimplexNoise::with_skipped_offsets(&mut LegacyRandom::new(1234), true),
            frozen: frozen.build().expect("fixed finite noise layers"),
            info: SimplexNoise::with_skipped_offsets(&mut LegacyRandom::new(2345), true),
        }
    })
}
impl TemperatureModifier {
    pub fn modify(self, position: BlockPos, base: f32) -> f32 {
        if self == Self::Frozen {
            let sources = noises();
            let x = f64::from(position.x);
            let z = f64::from(position.z);
            let frozen = f64::from(sources.frozen.get2(x * 0.05, z * 0.05) * 7.0);
            let info = f64::from(sources.info.get2(x * 0.2, z * 0.2));
            if frozen + info < 0.3 && f64::from(sources.info.get2(x * 0.09, z * 0.09)) < 0.8 {
                return 0.2;
            }
        }
        base
    }
}
impl BiomeTemperatureSettings {
    /// Decode the climate fields of a biome. Other biome fields are ignored,
    /// matching the native ClimateSettings map codec rather than the full biome.
    pub fn from_json(json: &str) -> Result<Self, DecodeError> {
        let object: Object = serde_json::from_str(json)?;
        let float = |name| {
            number_text(required(&object, name)?)?
                .parse::<f32>()
                .map_err(|_| DecodeError::Field(name))
        };
        let modifier = match optional(&object, "temperature_modifier") {
            None => TemperatureModifier::None,
            Some(raw) => match serde_json::from_str::<String>(raw.get())?.as_str() {
                "none" => TemperatureModifier::None,
                "frozen" => TemperatureModifier::Frozen,
                _ => return Err(DecodeError::Field("temperature_modifier")),
            },
        };
        Ok(Self {
            base_temperature: float("temperature")?,
            downfall: float("downfall")?,
            has_precipitation: serde_json::from_str(required(&object, "has_precipitation")?.get())?,
            modifier,
        })
    }

    pub fn height_adjusted(self, position: BlockPos, sea_level: i32) -> f32 {
        let base = self.modifier.modify(position, self.base_temperature);
        let start = sea_level.wrapping_add(17);
        if position.y > start {
            let variation = noises().height.get2(
                f64::from(position.x as f32 / 8.0),
                f64::from(position.z as f32 / 8.0),
            ) * 8.0;
            base - ((variation + position.y as f32 - start as f32) * 0.05 / 40.0)
        } else {
            base
        }
    }
}

/// Keep one instance per biome per worker, matching vanilla's thread-local map.
/// Hits do not reorder entries. The packed-position key deliberately omits sea
/// level, so a cached value survives queries with a different sea level.
#[derive(Debug)]
pub struct BiomeTemperature {
    settings: BiomeTemperatureSettings,
    values: HashMap<i64, f32>,
    order: VecDeque<i64>,
    /// Entries kept before the oldest goes: vanilla's 1,024 ([`Self::new`]).
    limit: usize,
}
/// Vanilla's `Biome.temperatureCache` size.
const VANILLA_TEMPERATURE_CACHE: usize = 1024;
/// A generation worker's cache size ([`BiomeTemperature::for_generation`]).
pub const GENERATION_TEMPERATURE_CACHE: usize = 256;
impl BiomeTemperature {
    pub fn new(settings: BiomeTemperatureSettings) -> Self {
        // Empty until used: a decoration region makes one per biome, and
        // most biomes never ask for a temperature.
        Self::with_limit(settings, VANILLA_TEMPERATURE_CACHE)
    }
    /// A generation worker's cache: [`GENERATION_TEMPERATURE_CACHE`] entries. Within one generator the
    /// sea level never changes, so a value is a pure function of the position and the cache only saves
    /// work; a worker keeps one per biome for good, which at vanilla's size held tens of MB.
    pub fn for_generation(settings: BiomeTemperatureSettings) -> Self {
        Self::with_limit(settings, GENERATION_TEMPERATURE_CACHE)
    }
    fn with_limit(settings: BiomeTemperatureSettings, limit: usize) -> Self {
        Self {
            settings,
            values: HashMap::new(),
            order: VecDeque::new(),
            limit,
        }
    }
    pub fn settings(&self) -> BiomeTemperatureSettings {
        self.settings
    }
    pub fn temperature(&mut self, position: BlockPos, sea_level: i32) -> f32 {
        let key = position.pack();
        if let Some(value) = self
            .values
            .get(&key)
            .copied()
            .filter(|value| !value.is_nan())
        {
            return value;
        }
        let value = self.settings.height_adjusted(position, sea_level);
        // NaN is both the missing-value sentinel and a possible stored value.
        // Recomputing an existing NaN at capacity still evicts the oldest key.
        if self.values.len() >= self.limit {
            let first = self.order.pop_front().expect("full cache has entries");
            self.values.remove(&first);
        }
        match self.values.entry(key) {
            Entry::Occupied(mut entry) => {
                entry.insert(value);
            }
            Entry::Vacant(entry) => {
                entry.insert(value);
                self.order.push_back(key);
            }
        }
        value
    }
    pub fn warm_enough_to_rain(&mut self, position: BlockPos, sea_level: i32) -> bool {
        self.temperature(position, sea_level) >= 0.15
    }
    pub fn cold_enough_to_snow(&mut self, position: BlockPos, sea_level: i32) -> bool {
        !self.warm_enough_to_rain(position, sea_level)
    }
    pub fn should_melt_frozen_ocean_iceberg_slightly(
        &mut self,
        position: BlockPos,
        sea_level: i32,
    ) -> bool {
        self.temperature(position, sea_level) > 0.1
    }
    pub fn precipitation_at(&mut self, position: BlockPos, sea_level: i32) -> Precipitation {
        if !self.settings.has_precipitation {
            Precipitation::None
        } else if self.cold_enough_to_snow(position, sea_level) {
            Precipitation::Snow
        } else {
            Precipitation::Rain
        }
    }
    pub fn cached_temperatures(&self) -> impl ExactSizeIterator<Item = (i64, f32)> + '_ {
        self.order.iter().map(|key| (*key, self.values[key]))
    }
}

/// Fixed biome-information noise shared by temperature and placement rules.
pub fn biome_info_noise(x: f64, z: f64) -> f32 {
    noises().info.get2(x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generation worker's smaller cache answers what vanilla's does, position by position, whatever it
    /// still holds: within one generator the value depends only on the position.
    #[test]
    fn a_generation_cache_answers_as_vanillas() {
        let settings = BiomeTemperatureSettings {
            base_temperature: 0.2,
            modifier: TemperatureModifier::Frozen,
            has_precipitation: true,
            downfall: 0.5,
        };
        let mut vanilla = BiomeTemperature::new(settings);
        let mut small = BiomeTemperature::for_generation(settings);
        let mut seed = 0x9e37_79b9_u32;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            // A small area, so positions come back after the small cache let them go.
            let at = BlockPos::new(
                (seed % 40) as i32 - 20,
                60 + (seed >> 8) as i32 % 60,
                (seed >> 16) as i32 % 40 - 20,
            );
            assert_eq!(
                small.temperature(at, 63).to_bits(),
                vanilla.temperature(at, 63).to_bits(),
                "{at:?}"
            );
        }
        assert_eq!(
            small.cached_temperatures().len(),
            GENERATION_TEMPERATURE_CACHE
        );
        assert_eq!(
            vanilla.cached_temperatures().len(),
            VANILLA_TEMPERATURE_CACHE
        );
    }
}
