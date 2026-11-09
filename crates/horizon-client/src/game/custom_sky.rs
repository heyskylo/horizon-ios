//! Custom skies (`horizon_client::custom_sky`, OptiFine's format): each frame, the layers of the
//! world's sky whose day, weather, biome and height hold at the camera, handed to the renderer,
//! which draws them over the vanilla sky. A pack without layers hands it none.
use super::*;
use horizon_client::custom_sky::{Conditions, Weather};

/// The layers' eased brightness between frames, and when the last frame was.
#[derive(Default)]
pub(super) struct CustomSkyView {
    state: horizon_client::custom_sky::State,
    last: Option<std::time::Instant>,
}

impl Game<'_> {
    /// Fills the renderer's custom sky layers for this frame; `camera` is the eye's position.
    /// Nothing for the title screen, the Nether (it has no sky) or a sky a shader pack draws.
    pub(super) fn custom_sky_frame(
        &mut self,
        camera: [f64; 3],
        sky: &horizon_client::environment::Sky,
        (rain, thunder): (f32, f32),
        title: bool,
    ) {
        let Some(mut draws) = self
            .renderer
            .as_mut()
            .map(|r| std::mem::take(r.custom_sky_draws()))
        else {
            return;
        };
        draws.clear();
        let world = match self.dim {
            Dim::Overworld => Some(0),
            Dim::End => Some(1),
            Dim::Nether => None,
        };
        let now = std::time::Instant::now();
        let seconds = self
            .custom_sky
            .last
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        if let Some(world) =
            world.filter(|&w| !title && self.settings.horizon.sky && self.pack.custom_sky.has(w))
        {
            let mut state = std::mem::take(&mut self.custom_sky.state);
            // Level.isThundering / isRaining.
            let weather = if thunder > 0.9 && rain > 0.2 {
                Weather::Thunder
            } else if rain > 0.2 {
                Weather::Rain
            } else {
                Weather::Clear
            };
            let conditions = Conditions {
                day_time: self.view_day_time(),
                weather,
                biome: self.biome_at(camera),
                y: camera[1].floor() as i32,
            };
            self.pack.custom_sky.draws(
                world,
                &conditions,
                sky.sun_angle / 360.0,
                seconds,
                &mut state,
                &mut draws,
            );
            self.custom_sky.state = state;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            *renderer.custom_sky_draws() = draws;
        }
    }
}
