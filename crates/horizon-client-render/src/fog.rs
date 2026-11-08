//! `FogRenderer` and its fog environments (atmospheric with rain fog, water
//! with water vision, lava, powder snow, blindness), the camera fog type
//! (`Camera.getFluidInCamera`) and the fog colour (`computeFogColor`: sunrise
//! tint, sky-colour mix, void darkness, water-vision brightening).
use crate::{
    biome::Biomes,
    blocks::{BlockFacts, Fluid},
    camera::NearPlane,
    environment::Sky,
    world::World,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use std::path::Path;

/// `FogType` of the camera position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogType {
    Atmospheric,
    Water,
    Lava,
    PowderSnow,
}

/// The fog uniform (`FogData`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FogData {
    pub color: [f32; 4],
    pub environmental_start: f32,
    pub environmental_end: f32,
    pub render_distance_start: f32,
    pub render_distance_end: f32,
    pub sky_end: f32,
    pub cloud_end: f32,
}

/// Per-biome water fog end multipliers (`visual/water_fog_end_distance`).
#[derive(Default)]
pub struct FogTables {
    water_fog_end: Vec<f32>,
}
impl FogTables {
    pub fn load(root: &Path, biomes: &Biomes) -> Self {
        let mut water_fog_end = vec![1.0; biomes.list.len()];
        for (name, &id) in &biomes.index {
            let path = root.join(format!(
                "data/minecraft/worldgen/biome/{}.json",
                name.trim_start_matches("minecraft:")
            ));
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let v = &json["attributes"]["minecraft:visual/water_fog_end_distance"];
            if v["modifier"].as_str() == Some("multiply")
                && let (Some(f), Some(slot)) =
                    (v["argument"].as_f64(), water_fog_end.get_mut(id as usize))
            {
                *slot = f as f32;
            }
        }
        Self { water_fog_end }
    }
}

/// Fog state that evolves over time.
#[derive(Clone, Debug)]
pub struct FogState {
    /// `LocalPlayer.waterVisionTime` (0..600).
    pub water_vision_time: i32,
    /// `AtmosphericFogEnvironment.rainFogMultiplier`.
    pub rain_fog_multiplier: f32,
    /// `LightmapRenderStateExtractor.blockLightFlicker`.
    pub block_light_flicker: f32,
    /// `LightmapRenderStateExtractor.random`: `RandomSource.create()`.
    flicker_random: LegacyRandom,
}
impl Default for FogState {
    fn default() -> Self {
        Self {
            water_vision_time: 0,
            rain_fog_multiplier: 0.0,
            block_light_flicker: 0.0,
            flicker_random: crate::random_source::create(),
        }
    }
}
impl FogState {
    /// `LightmapRenderStateExtractor.tick`: the torch light flickers.
    pub fn tick_flicker(&mut self) {
        let r = &mut self.flicker_random;
        let (a, b, c, d) = (
            r.next_float(),
            r.next_float(),
            r.next_float(),
            r.next_float(),
        );
        self.block_light_flicker += (a - b) * c * d * 0.1;
        self.block_light_flicker *= 0.9;
    }
    /// `LocalPlayer.aiStep`: water vision ramps up while the eye is in water, ten times faster
    /// for a spectator.
    pub fn tick(&mut self, eye_in_water: bool, spectator: bool) {
        self.water_vision_time = if eye_in_water {
            (self.water_vision_time + if spectator { 10 } else { 1 }).min(600)
        } else {
            (self.water_vision_time - 10).max(0)
        };
    }
    /// `LocalPlayer.getWaterVision`.
    pub fn water_vision(&self, eye_in_water: bool) -> f32 {
        if !eye_in_water {
            return 0.0;
        }
        let t = self.water_vision_time as f32;
        if t >= 600.0 {
            return 1.0;
        }
        let a = (t / 100.0).clamp(0.0, 1.0);
        let b = if t < 100.0 {
            0.0
        } else {
            ((t - 100.0) / 500.0).clamp(0.0, 1.0)
        };
        a * 0.6 + b * 0.399_999_98
    }
}

/// `FluidState.getHeightForCamera`.
fn camera_fluid_height(world: &World, facts: &BlockFacts, b: [i32; 3]) -> Option<(Fluid, f64)> {
    let s = facts.get(world.block(b[0], b[1], b[2]));
    if s.fluid == Fluid::None {
        return None;
    }
    let above = facts.get(world.block(b[0], b[1] + 1, b[2]));
    let source = s.fluid_level >= 8 && !s.fluid_falling;
    let height = if (source && above.full_collision) || above.fluid == s.fluid {
        1.0
    } else {
        f64::from(s.fluid_level) / 9.0
    };
    Some((s.fluid, height))
}
/// `Camera.getFluidInCamera`: water at the camera, then lava or powder snow
/// at the near plane's centre and corners.
pub fn fog_type(world: &World, facts: &BlockFacts, camera: [f64; 3], near: &NearPlane) -> FogType {
    camera_fog_type(
        camera,
        near,
        |b| camera_fluid_height(world, facts, b),
        |b| &*facts.get(world.block(b[0], b[1], b[2])).name == "minecraft:powder_snow",
    )
}
/// `fog_type` over a block lookup: the fluid and its camera height, and
/// whether the block is powder snow.
fn camera_fog_type(
    camera: [f64; 3],
    near: &NearPlane,
    fluid: impl Fn([i32; 3]) -> Option<(Fluid, f64)>,
    powder_snow: impl Fn([i32; 3]) -> bool,
) -> FogType {
    let block = |p: [f64; 3]| {
        [
            p[0].floor() as i32,
            p[1].floor() as i32,
            p[2].floor() as i32,
        ]
    };
    let b = block(camera);
    if let Some((Fluid::Water, h)) = fluid(b)
        && camera[1] < f64::from(b[1]) + h
    {
        return FogType::Water;
    }
    for point in near.points() {
        let p: [f64; 3] = std::array::from_fn(|i| camera[i] + point[i]);
        let b = block(p);
        match fluid(b) {
            Some((Fluid::Lava, h)) if p[1] <= f64::from(b[1]) + h => return FogType::Lava,
            Some((Fluid::Lava, _)) => {}
            _ if powder_snow(b) => return FogType::PowderSnow,
            _ => {}
        }
    }
    FogType::Atmospheric
}

/// Everything `FogRenderer.setupFog` reads for one frame.
pub struct FogInputs<'a> {
    pub fog_type: FogType,
    pub sky: &'a Sky,
    /// Biome water fog colour (`visual/water_fog_color`).
    pub water_fog_color: [f32; 3],
    pub biome: u16,
    pub render_distance_chunks: i32,
    /// Extend ordinary Overworld atmosphere for terrain beyond vanilla's range.
    pub distant_terrain: bool,
    /// The Cloud Distance option, in chunks.
    pub cloud_range_chunks: i32,
    pub camera_y: f64,
    pub min_y: i32,
    /// Flat worlds use a 1-block void darkness range.
    pub flat: bool,
    pub forward: [f32; 3],
    pub rain: f32,
    pub thunder: f32,
    /// Dimensions with a sky disc (overworld): sunrise tint and sky mix apply.
    pub skybox: bool,
    /// Blindness: (remaining duration ticks, infinite).
    pub blindness: Option<(i32, bool)>,
    /// The darkness effect's blend factor, while the effect is on.
    pub darkness: Option<f32>,
    /// `GameRenderer.bossOverlayWorldDarkening`: a wither's bar dims the fog.
    pub darken_world: f32,
    /// `BossHealthOverlay.shouldCreateWorldFog`: a boss bar with
    /// `createWorldFog` is shown (the Ender Dragon's).
    pub boss_world_fog: bool,
    /// The camera entity is a spectator (`LavaFogEnvironment`).
    pub spectator: bool,
    /// The camera entity has Fire Resistance (`LavaFogEnvironment`), or PvP & HUD's clear lava
    /// asks for its fog.
    pub fire_resistance: bool,
    /// PvP & HUD's clear water: the water vision `FogState` reaches after 600 ticks under water.
    pub full_water_vision: bool,
    /// `GameRenderer.nightVisionScale` while Night Vision is on without
    /// Darkness, else 0 (`FogRenderer.computeFogColor`'s brightenFactor).
    pub night_vision: f32,
}

/// `FogRenderer.setupFog` + `computeFogColor`.
pub fn setup_fog(tables: &FogTables, state: &FogState, input: &FogInputs) -> FogData {
    let rd = input.render_distance_chunks as f32 * 16.0;
    let sky = input.sky;
    let eye_in_water = input.fog_type == FogType::Water;
    let water_vision = if input.full_water_vision && eye_in_water {
        1.0
    } else {
        state.water_vision(eye_in_water)
    };
    // Base colour of the applicable environment.
    let mut color = match input.fog_type {
        FogType::Lava => rgb(0x99_1A00),
        FogType::PowderSnow => rgb(0x9F_BACC),
        FogType::Water => input.water_fog_color,
        FogType::Atmospheric => {
            let mut fog = sky.fog_color;
            if input.skybox && input.render_distance_chunks >= 4 {
                let sun_x = if sky.sun_angle.to_radians().sin() > 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let looking = input.forward[0] * sun_x;
                let alpha = sky.sunrise_color[3];
                if looking > 0.0 && alpha > 0.0 {
                    let t = looking * alpha;
                    fog = std::array::from_fn(|i| fog[i] + (sky.sunrise_color[i] - fog[i]) * t);
                }
            }
            let mut sky_color = sky.sky_color;
            if input.rain > 0.0 {
                let m = 1.0 - input.rain * 0.5;
                let mb = 1.0 - input.rain * 0.4;
                sky_color = [sky_color[0] * m, sky_color[1] * m, sky_color[2] * mb];
            }
            if input.thunder > 0.0 {
                let m = 1.0 - input.thunder * 0.5;
                sky_color = sky_color.map(|c| c * m);
            }
            let sky_fog_end = (512.0f32 / 16.0).min(input.render_distance_chunks as f32);
            let mix = (sky_fog_end / 32.0).clamp(0.0, 1.0).mul_add(0.75, 0.25);
            let mix = 1.0 - mix.powf(0.25);
            std::array::from_fn(|i| fog[i] + (sky_color[i] - fog[i]) * mix)
        }
    };
    // Void darkness and blindness.
    let range = if input.flat { 1.0 } else { 32.0 };
    let mut darkness =
        ((range + input.min_y as f32 - input.camera_y as f32) / range).clamp(0.0, 1.0);
    if let Some((duration, infinite)) = input.blindness {
        darkness = if !infinite && duration < 20 {
            darkness.max(duration as f32 / 20.0)
        } else {
            1.0
        };
    } else if let Some(blend) = input.darkness {
        // DarknessFogEnvironment.getModifiedDarkness.
        darkness = darkness.max(blend);
    }
    if darkness > 0.0 && !matches!(input.fog_type, FogType::Lava | FogType::PowderSnow) {
        let brightness = (1.0 - darkness) * (1.0 - darkness);
        color = color.map(|c| c * brightness);
    }
    // FogRenderer.computeFogColor: a boss bar's world darkening.
    if input.darken_world > 0.0 {
        let t = input.darken_world;
        color[0] += (color[0] * 0.7 - color[0]) * t;
        color[1] += (color[1] * 0.6 - color[1]) * t;
        color[2] += (color[2] * 0.6 - color[2]) * t;
    }
    let brighten = if eye_in_water {
        water_vision
    } else {
        input.night_vision
    };
    if color.iter().all(|&c| c != 0.0) {
        let scale = 1.0 / color[0].max(color[1]).max(color[2]);
        color = color.map(|c| c + (c * scale - c) * brighten);
    }
    let mut fog = FogData {
        color: [color[0], color[1], color[2], 1.0],
        environmental_start: 0.0,
        environmental_end: 1024.0,
        render_distance_start: 0.0,
        render_distance_end: rd,
        sky_end: rd.min(512.0),
        cloud_end: 2048.0,
    };
    match input.fog_type {
        FogType::Lava => {
            // LavaFogEnvironment.setupFog.
            (fog.environmental_start, fog.environmental_end) = if input.spectator {
                (-8.0, rd * 0.5)
            } else if input.fire_resistance {
                (0.0, 5.0)
            } else {
                (0.25, 1.0)
            };
            fog.sky_end = fog.environmental_end;
            fog.cloud_end = fog.environmental_end;
        }
        FogType::PowderSnow => {
            // PowderedSnowFogEnvironment.setupFog.
            (fog.environmental_start, fog.environmental_end) = if input.spectator {
                (-8.0, rd * 0.5)
            } else {
                (0.0, 2.0)
            };
            fog.sky_end = fog.environmental_end;
            fog.cloud_end = fog.environmental_end;
        }
        _ if input.blindness.is_some() => {
            let (duration, infinite) = input.blindness.unwrap_or((0, true));
            let distance = if infinite {
                5.0
            } else {
                rd + (5.0 - rd) * (duration as f32 / 20.0).min(1.0)
            };
            fog.environmental_start = distance * 0.25;
            fog.environmental_end = distance;
            fog.sky_end = distance * 0.8;
            fog.cloud_end = distance * 0.8;
        }
        _ if input.darkness.is_some() => {
            // DarknessFogEnvironment.setupFog: closing in to 15 blocks.
            let blend = input.darkness.unwrap_or(0.0);
            let distance = rd + (15.0 - rd) * blend;
            fog.environmental_start = distance * 0.75;
            fog.environmental_end = distance;
            fog.sky_end = distance;
            fog.cloud_end = distance;
        }
        FogType::Water => {
            fog.environmental_start = -8.0;
            let factor = tables
                .water_fog_end
                .get(input.biome as usize)
                .copied()
                .unwrap_or(1.0);
            fog.environmental_end = 96.0 * factor * water_vision.max(0.25);
            fog.sky_end = fog.environmental_end;
            fog.cloud_end = fog.environmental_end;
        }
        FogType::Atmospheric => {
            fog.environmental_start = sky.fog_start + -160.0 * state.rain_fog_multiplier;
            let min_end = sky.fog_end.min(96.0);
            fog.environmental_end = min_end.max(sky.fog_end + -256.0 * state.rain_fog_multiplier);
            fog.sky_end = rd.min(512.0);
            fog.cloud_end = ((input.cloud_range_chunks * 16) as f32).min(sky.cloud_fog_end);
            if input.boss_world_fog {
                fog.environmental_start = fog.environmental_start.min(10.0);
                fog.environmental_end = fog.environmental_end.min(96.0);
                fog.sky_end = fog.environmental_end;
                fog.cloud_end = fog.environmental_end;
            }
        }
    }
    let span = (rd / 10.0).clamp(4.0, 64.0);
    fog.render_distance_start = rd - span;
    fog.render_distance_end = rd;
    if input.distant_terrain
        && input.skybox
        && input.fog_type == FogType::Atmospheric
        && input.blindness.is_none()
        && input.darkness.is_none()
        && !input.boss_world_fog
        && sky.fog_end >= 1024.0
    {
        let scale = (rd / 1024.0).max(1.0);
        fog.environmental_start *= scale;
        fog.environmental_end *= scale;
    }
    fog
}
/// Horizon's Fog option turned off (`horizon-graphics.txt` `fog:false`):
/// no distance fog, in any dimension: the dimension's and biome's own fog
/// (`visual/fog_start_distance`, `fog_end_distance`: the Nether's ends at 96
/// blocks) and the render-distance fog. The fog of the fluid or powder snow
/// the camera is in, blindness, darkness and a boss's world fog keep their
/// limits. Vanilla has no such option.
pub fn clear_distance_fog(fog: &mut FogData, input: &FogInputs) {
    if input.fog_type == FogType::Atmospheric
        && input.darkness.is_none()
        && !input.boss_world_fog
        && input.blindness.is_none()
    {
        (fog.render_distance_start, fog.render_distance_end) = (1e9, 2e9);
        (fog.environmental_start, fog.environmental_end) = (1e9, 2e9);
    }
}
/// What `AtmosphericFogEnvironment.updateRainFogState` eases the rain fog toward: the rain, where
/// the sky reaches the eye, halved in a biome without precipitation.
pub fn rain_fog_target(rain: f32, sky_light: u8, biome_rains: bool) -> f32 {
    let sky = ((f32::from(sky_light) - 8.0) / 7.0).clamp(0.0, 1.0);
    rain * sky * if biome_rains { 1.0 } else { 0.5 }
}
/// Whether the frame draws no sky, only the clear to the fog colour: `LevelRenderer.addSkyPass`
/// leaves the sky out in lava and powder snow and while blindness or darkness is on (`Camera`'s
/// `doesMobEffectBlockSky`), and `GameRenderer.renderLevel` gives `LevelRenderer.render`
/// `shouldRenderSky` false while a boss bar makes world fog (the Ender Dragon's).
pub fn sky_pass_skipped(fog_type: FogType, effect_blocks_sky: bool, boss_world_fog: bool) -> bool {
    matches!(fog_type, FogType::Lava | FogType::PowderSnow) || effect_blocks_sky || boss_world_fog
}
/// `AtmosphericFogEnvironment.updateRainFogState`.
pub fn update_rain_fog(
    state: &mut FogState,
    rain: f32,
    sky_light: u8,
    biome_rains: bool,
    delta_ticks: f32,
) {
    let target = rain_fog_target(rain, sky_light, biome_rains);
    state.rain_fog_multiplier += (target - state.rain_fog_multiplier) * delta_ticks * 0.2;
}
/// The rain fog at its target at once: a frozen world under `HORIZON_STILL`, where the rain sits
/// at its level and easing would wait for ticks that do not come.
pub fn settle_rain_fog(state: &mut FogState, rain: f32, sky_light: u8, biome_rains: bool) {
    state.rain_fog_multiplier = rain_fog_target(rain, sky_light, biome_rains);
}
fn rgb(c: u32) -> [f32; 3] {
    [
        (c >> 16 & 255) as f32 / 255.0,
        (c >> 8 & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(sky: &Sky) -> FogInputs<'_> {
        FogInputs {
            fog_type: FogType::Atmospheric,
            sky,
            water_fog_color: [0.0, 0.0, 0.2],
            biome: 0,
            render_distance_chunks: 12,
            distant_terrain: false,
            cloud_range_chunks: 128,
            camera_y: 70.0,
            min_y: -64,
            flat: false,
            forward: [0.0, 0.0, 1.0],
            rain: 0.0,
            thunder: 0.0,
            skybox: true,
            blindness: None,
            darkness: None,
            darken_world: 0.0,
            boss_world_fog: false,
            spectator: false,
            fire_resistance: false,
            full_water_vision: false,
            night_vision: 0.0,
        }
    }
    #[test]
    fn lava_fog_starts_when_a_near_plane_corner_dips_under_the_surface() {
        // Flowing lava (amount 4) on y 64: the camera sees its surface at 64 + 4/9.
        let surface = 64.0 + 4.0 / 9.0;
        let fluid = |b: [i32; 3]| (b[1] == 64).then_some((Fluid::Lava, 4.0 / 9.0));
        let fog = |y: f64, pitch: f32| {
            let mut camera = crate::camera::Camera::default();
            camera.set_rotation(0.0, pitch);
            let near = camera.near_plane(70.0, 1600.0, 900.0);
            camera_fog_type([0.5, y, 0.5], &near, fluid, |_| false)
        };
        // Looking level, the near plane reaches tan(35 degrees) * 0.05 = 0.035 below the eye.
        assert_eq!(fog(surface + 0.02, 0.0), FogType::Lava);
        assert_eq!(fog(surface + 0.05, 0.0), FogType::Atmospheric);
        // The eye itself is not tested: looking up, the whole plane is above it.
        assert_eq!(fog(surface - 0.01, -90.0), FogType::Atmospheric);
    }
    #[test]
    fn the_dragon_fights_world_fog_leaves_out_the_sky() {
        use FogType::*;
        // GameRenderer.renderLevel: no sky pass under a createWorldFog boss bar, in clear air
        // as in water.
        assert!(sky_pass_skipped(Atmospheric, false, true));
        assert!(sky_pass_skipped(Water, false, true));
        // LevelRenderer.addSkyPass's own conditions.
        assert!(sky_pass_skipped(Lava, false, false));
        assert!(sky_pass_skipped(PowderSnow, false, false));
        assert!(sky_pass_skipped(Atmospheric, true, false));
        assert!(!sky_pass_skipped(Atmospheric, false, false));
        assert!(!sky_pass_skipped(Water, false, false));
    }
    #[test]
    fn boss_world_fog_closes_in() {
        // AtmosphericFogEnvironment.setupFog under a createWorldFog boss bar.
        let sky = Sky {
            fog_end: 1024.0,
            cloud_fog_end: 2048.0,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&sky);
        input.boss_world_fog = true;
        let fog = setup_fog(&tables, &FogState::default(), &input);
        assert_eq!(
            (fog.environmental_start, fog.environmental_end),
            (0.0, 96.0)
        );
        assert_eq!((fog.sky_end, fog.cloud_end), (96.0, 96.0));
        // Under water the water environment applies instead.
        input.fog_type = FogType::Water;
        let water = setup_fog(&tables, &FogState::default(), &input);
        assert_eq!(water.environmental_start, -8.0);
    }
    #[test]
    fn fog_off_clears_the_nether_fog_too() {
        // The Nether's visual/fog_start_distance and fog_end_distance (10, 96)
        // end before the render distance; the Fog option still clears them.
        let nether = Sky {
            fog_start: 10.0,
            fog_end: 96.0,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&nether);
        input.skybox = false;
        let mut fog = setup_fog(&tables, &FogState::default(), &input);
        assert_eq!(fog.environmental_end, 96.0);
        clear_distance_fog(&mut fog, &input);
        assert!(fog.environmental_start >= 1e9 && fog.render_distance_start >= 1e9);
    }
    #[test]
    fn fog_off_keeps_fluid_and_effect_fog() {
        let sky = Sky {
            fog_start: 10.0,
            fog_end: 96.0,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let kept = |input: &FogInputs| {
            let mut fog = setup_fog(&tables, &FogState::default(), input);
            let before = fog;
            clear_distance_fog(&mut fog, input);
            fog == before
        };
        for fog_type in [FogType::Lava, FogType::PowderSnow, FogType::Water] {
            let mut input = inputs(&sky);
            input.fog_type = fog_type;
            assert!(kept(&input), "{fog_type:?}");
        }
        let mut input = inputs(&sky);
        input.blindness = Some((100, false));
        assert!(kept(&input));
        let mut input = inputs(&sky);
        input.darkness = Some(0.5);
        assert!(kept(&input));
        let mut input = inputs(&sky);
        input.boss_world_fog = true;
        assert!(kept(&input));
    }
    #[test]
    fn boss_darkening_dims_fog_color() {
        // FogRenderer.computeFogColor: lerp toward 70/60/60% of each channel.
        let sky = Sky {
            fog_color: [1.0, 1.0, 1.0],
            sky_color: [1.0, 1.0, 1.0],
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&sky);
        input.skybox = false;
        input.darken_world = 1.0;
        let fog = setup_fog(&tables, &FogState::default(), &input);
        assert!((fog.color[0] - 0.7).abs() < 1.0e-5);
        assert!((fog.color[1] - 0.6).abs() < 1.0e-5);
        assert!((fog.color[2] - 0.6).abs() < 1.0e-5);
    }
    #[test]
    fn void_darkness_starts_32_above_the_floor() {
        let sky = Sky {
            fog_end: 1024.0,
            fog_color: [0.5; 3],
            sky_color: [0.5; 3],
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&sky);
        // The End's floor is y 0: y 16 is halfway into the darkness.
        input.min_y = 0;
        input.camera_y = 16.0;
        let fog = setup_fog(&tables, &FogState::default(), &input);
        assert_eq!(fog.color[0], 0.5 * 0.25);
        input.camera_y = 32.0;
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).color[0],
            0.5
        );
    }
    #[test]
    fn render_distance_fog_span() {
        let sky = Sky {
            fog_end: 1024.0,
            fog_color: [0.5; 3],
            sky_color: [0.5; 3],
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let fog = setup_fog(&tables, &FogState::default(), &inputs(&sky));
        // 12 chunks = 192 blocks, span clamp(19.2, 4, 64).
        assert!((fog.render_distance_start - (192.0 - 19.2)).abs() < 1e-4);
        assert_eq!(fog.render_distance_end, 192.0);
        assert_eq!(fog.sky_end, 192.0);
    }
    #[test]
    fn distant_atmosphere_extends_without_extending_environmental_overrides() {
        let sky = Sky {
            fog_end: 1024.,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.],
        };
        let mut input = inputs(&sky);
        input.render_distance_chunks = 1024;
        input.distant_terrain = true;
        let fog = setup_fog(&tables, &FogState::default(), &input);
        assert_eq!(fog.environmental_end, 16384.);
        input.fog_type = FogType::Water;
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).environmental_end,
            24.
        );
        input.fog_type = FogType::Atmospheric;
        input.darkness = Some(1.);
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).environmental_end,
            15.
        );
        input.darkness = None;
        input.boss_world_fog = true;
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).environmental_end,
            96.
        );
    }
    #[test]
    fn cloud_fog_ends_at_the_cloud_distance() {
        let sky = Sky {
            fog_end: 1024.0,
            cloud_fog_end: 2048.0,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&sky);
        input.render_distance_chunks = 2;
        input.cloud_range_chunks = 64;
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).cloud_end,
            1024.0
        );
        input.cloud_range_chunks = 128;
        assert_eq!(
            setup_fog(&tables, &FogState::default(), &input).cloud_end,
            2048.0
        );
    }
    #[test]
    fn water_vision_ramps() {
        let mut s = FogState::default();
        for _ in 0..100 {
            s.tick(true, false);
        }
        assert!((s.water_vision(true) - 0.6).abs() < 1e-6);
        for _ in 0..500 {
            s.tick(true, false);
        }
        assert_eq!(s.water_vision(true), 1.0);
        assert_eq!(s.water_vision(false), 0.0);
    }
    #[test]
    fn clear_water_sees_as_far_as_after_thirty_seconds_under_water() {
        let sky = Sky::default();
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let mut input = inputs(&sky);
        input.fog_type = FogType::Water;
        // Just in: vanilla's water vision is 0, so the fog ends at a quarter of 96 blocks.
        let mut state = FogState::default();
        state.tick(true, false);
        let first = setup_fog(&tables, &state, &input);
        assert_eq!(first.environmental_end, 24.0);
        for _ in 0..600 {
            state.tick(true, false);
        }
        let settled = setup_fog(&tables, &state, &input);
        assert_eq!(settled.environmental_end, 96.0);
        input.full_water_vision = true;
        let mut fresh = FogState::default();
        fresh.tick(true, false);
        let clear = setup_fog(&tables, &fresh, &input);
        assert_eq!(clear.environmental_end, settled.environmental_end);
        assert_eq!(clear.color, settled.color);
    }
    /// Settling puts the rain fog at its target exactly, where a tick of easing moves it a fifth
    /// of the way, as before.
    #[test]
    fn settled_rain_fog_is_its_target_and_easing_is_unchanged() {
        assert_eq!(
            rain_fog_target(1.0, 15, false),
            0.5,
            "a dry biome halves it"
        );
        assert_eq!(rain_fog_target(1.0, 8, true), 0.0, "no sky, no rain fog");
        let start = FogState {
            rain_fog_multiplier: 0.1,
            ..FogState::default()
        };
        let mut settled = start.clone();
        settle_rain_fog(&mut settled, 0.5, 15, true);
        assert_eq!(settled.rain_fog_multiplier, 0.5);
        let mut eased = start;
        update_rain_fog(&mut eased, 0.5, 15, true, 1.0);
        assert_eq!(
            eased.rain_fog_multiplier.to_bits(),
            (0.1f32 + (0.5f32 - 0.1) * 1.0 * 0.2).to_bits()
        );
    }
    #[test]
    fn rain_fog_pulls_fog_in() {
        let sky = Sky {
            fog_end: 1024.0,
            ..Sky::default()
        };
        let tables = FogTables {
            water_fog_end: vec![1.0],
        };
        let state = FogState {
            rain_fog_multiplier: 1.0,
            ..FogState::default()
        };
        let fog = setup_fog(&tables, &state, &inputs(&sky));
        assert_eq!(fog.environmental_start, -160.0);
        assert_eq!(fog.environmental_end, 768.0);
    }

    #[test]
    fn a_spectator_sees_clearly_under_water_ten_times_sooner() {
        let mut swimmer = FogState::default();
        let mut spectator = FogState::default();
        for _ in 0..60 {
            swimmer.tick(true, false);
            spectator.tick(true, true);
        }
        // LocalPlayer.getWaterVision after 60 ticks: 60 and 600 ticks of vision.
        assert!((swimmer.water_vision(true) - 0.36).abs() < 1e-6);
        assert_eq!(spectator.water_vision(true), 1.0);
        // Out of the water both lose it 10 a tick.
        spectator.tick(false, true);
        assert_eq!(spectator.water_vision_time, 590);
    }
}
