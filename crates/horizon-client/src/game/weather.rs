//! Weather and sky light for the player's level: whether it can have weather, the rain and thunder
//! levels, where it rains or snows (`Level.isRainingAt`, the motion-blocking height), the weather's
//! effects each tick and `Level.getSkyDarken` with `isDarkOutside`.
use super::*;

impl Game<'_> {
    /// `Level.isDarkOutside`.
    pub(super) fn dark_outside(&self) -> bool {
        !self.dimension_type().has_fixed_time && self.sky_darken() >= 4
    }

    pub(super) fn can_have_weather(&self) -> bool {
        level_can_have_weather(
            self.world.shape.sky_light,
            self.world.shape.has_ceiling,
            self.dim == Dim::End,
        )
    }

    /// The rain and thunder levels (`Level.getRainLevel`, `getThunderLevel`):
    /// the world's, where the level can have weather, else none.
    pub(super) fn weather_levels(&self, partial: f32) -> (f32, f32) {
        if self.can_have_weather() {
            self.world.weather.levels(partial)
        } else {
            (0.0, 0.0)
        }
    }

    pub(super) fn sky_darken(&self) -> i32 {
        if self.dim == Dim::Nether {
            return 11;
        }
        if self.dim == Dim::End {
            let sky = self
                .res
                .end_environment()
                .sample(&horizon_client::environment::Sample {
                    day_time: self.world.day_time,
                    ..Default::default()
                });
            // Level.updateSkyBrightness truncates (`(int)`), see `environment::sky_darken`.
            return (15.0 - 15.0 * sky.sky_light_level) as i32;
        }
        let (rain, thunder) = self.weather_levels(1.0);
        // Sampled once per (time, weather): every mob asks each tick.
        let key = (self.world.day_time, rain.to_bits(), thunder.to_bits());
        if let Some((k, darken)) = self.sky_darken_memo.get()
            && k == key
        {
            return darken;
        }
        let darken = horizon_client::environment::sky_darken(
            self.res.environment(),
            self.world.day_time,
            rain,
            thunder,
        );
        self.sky_darken_memo.set(Some((key, darken)));
        darken
    }

    /// Whether precipitation at a column is snow (Biome.coldEnoughToSnow:
    /// height adjusted temperature) or none (no precipitation).
    pub(super) fn precipitation(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        self.res
            .biomes()
            .get(chunk.biome(x, y, z))
            .snows_at(x, y, z)
    }

    /// `Level.isRainingAt`: it rains, the sky light is 15, nothing that stops
    /// motion is above, and the biome's precipitation there is rain.
    pub(super) fn raining_at(&self, p: BlockPos) -> bool {
        self.weather_levels(1.0).0 > 0.2
            && self.world.light(p.x, p.y, p.z) >> 4 == 15
            && self.rain_height(p.x, p.z) <= p.y
            && self.precipitation(p.x, p.y, p.z) == Some(false)
    }

    /// `getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)`: one above the
    /// highest block in `blocks_motion_in_heightmap` or holding a fluid.
    pub(super) fn rain_height(&self, x: i32, z: i32) -> i32 {
        render_tick::motion_blocking_height(&self.world, &self.pack.assets.properties, x, z)
    }

    pub(super) fn tick_weather_effects(&mut self) {
        if self.flash > 0 {
            self.flash -= 1;
        }
        // The rain sounds come with the rain splashes (`tick_particles`).
    }
}
