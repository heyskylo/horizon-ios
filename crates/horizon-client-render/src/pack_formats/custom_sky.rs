//! OptiFine custom skies (`horizon_optifine::sky`) on the engine's sky layers
//! ([`crate::custom_sky`]): a pack's layers with their decoded textures, read once with the pack,
//! and each frame's choice of layers, their brightness and rotation. A pack without
//! `optifine/sky` has none, and the sky stays vanilla.

use std::path::Path;

use glam::{Mat4, Vec3};
use horizon_optifine::sky::{Blend, Conditions, Now, Skies, SkyLayer, Transition};

use crate::custom_sky::{self as engine, Draw, Image, SkyLayers, State};

/// The pack's layers, by world, with one texture per layer (`images[i]` for the `i`th layer
/// over all worlds, in `worlds` order).
#[derive(Default)]
pub struct CustomSky {
    worlds: Vec<(i32, Vec<SkyLayer>)>,
    pub images: Vec<Image>,
}

/// Textures past this many texels in all are left out (a skybox is a few megapixels at most).
const TEXEL_BUDGET: u64 = 64 << 20;

impl CustomSky {
    /// The custom skies of the overlay at `root`. A layer whose texture does not decode is
    /// dropped with a warning.
    pub fn load(root: &Path) -> Self {
        let skies = Skies::scan(root);
        let mut out = Self::default();
        let mut texels = 0u64;
        for (world, sky) in skies.worlds {
            let mut layers = Vec::new();
            for layer in sky.layers {
                match crate::hud::decode(&root.join(&layer.source)) {
                    Ok((width, height, rgba))
                        if texels + u64::from(width) * u64::from(height) <= TEXEL_BUDGET =>
                    {
                        texels += u64::from(width) * u64::from(height);
                        out.images.push(Image {
                            width,
                            height,
                            rgba,
                        });
                        layers.push(layer);
                    }
                    Ok(_) => log::warn!("{}: past the custom sky's texture budget", layer.source),
                    Err(e) => log::warn!("{}: {e:#}", layer.source),
                }
            }
            if !layers.is_empty() {
                out.worlds.push((world, layers));
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }
}

/// The engine's sky blend for OptiFine's.
fn blend(blend: Blend) -> engine::Blend {
    match blend {
        Blend::Add => engine::Blend::Add,
        Blend::Subtract => engine::Blend::Subtract,
        Blend::Multiply => engine::Blend::Multiply,
        Blend::Dodge => engine::Blend::Dodge,
        Blend::Burn => engine::Blend::Burn,
        Blend::Screen => engine::Blend::Screen,
        Blend::Replace => engine::Blend::Replace,
        Blend::Overlay => engine::Blend::Overlay,
        Blend::Alpha => engine::Blend::Alpha,
    }
}

impl SkyLayers for CustomSky {
    fn images(&self) -> &[Image] {
        &self.images
    }

    /// Whether `world` has layers (`0` the Overworld, `1` the End).
    fn has(&self, world: i32) -> bool {
        self.worlds.iter().any(|(w, _)| *w == world)
    }

    /// The layers whose conditions hold, eased over their `transition`, at the brightness of
    /// their fade times.
    fn draws(
        &self,
        world: i32,
        conditions: &engine::Conditions,
        sun_turns: f32,
        seconds: f32,
        state: &mut State,
        out: &mut Vec<Draw>,
    ) {
        let conditions = &Conditions {
            day_time: conditions.day_time,
            weather: match conditions.weather {
                engine::Weather::Clear => Now::Clear,
                engine::Weather::Rain => Now::Rain,
                engine::Weather::Thunder => Now::Thunder,
            },
            biome: conditions.biome,
            y: conditions.y,
        };
        out.clear();
        let mut first = 0;
        let mut found = None;
        for (w, layers) in &self.worlds {
            if *w == world {
                found = Some(layers);
                break;
            }
            first += layers.len();
        }
        let Some(layers) = found else {
            state.world = None;
            return;
        };
        // A new world (or pack) starts each layer where its conditions put it: no fade in.
        let snap = state.world != Some(world) || state.eased.len() != layers.len();
        if snap {
            state.world = Some(world);
            state.eased.clear();
            state.eased.resize(layers.len(), 0.0);
        }
        for (i, (layer, eased)) in layers.iter().zip(&mut state.eased).enumerate() {
            let target = if layer.shows(conditions) { 1.0 } else { 0.0 };
            let shown = if snap {
                *eased = target;
                target
            } else {
                let mut t = Transition { value: *eased };
                *eased = t.step(target, seconds, layer.transition);
                *eased
            };
            let brightness = shown * layer.brightness(conditions.day_time);
            if brightness <= 0.0 {
                continue;
            }
            let turns = layer.turns(conditions.day_time, sun_turns);
            let rotation = if turns == 0.0 {
                Mat4::IDENTITY
            } else {
                Mat4::from_axis_angle(Vec3::from_array(layer.axis), turns * std::f32::consts::TAU)
            };
            out.push(Draw {
                image: (first + i) as u16,
                blend: blend(layer.blend),
                brightness,
                rotation,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_sky::{Conditions, Weather};

    fn pack(files: &[(&str, &str)]) -> (std::path::PathBuf, CustomSky) {
        let root = std::env::temp_dir().join(format!(
            "horizon-custom-sky-{}-{}",
            std::process::id(),
            files.len()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("assets/minecraft/optifine/sky/world0");
        std::fs::create_dir_all(&dir).unwrap();
        let png = {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, 6, 4);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255].repeat(24))
                .unwrap();
            writer.finish().unwrap();
            bytes
        };
        for (name, text) in files {
            std::fs::write(dir.join(format!("{name}.properties")), text).unwrap();
            std::fs::write(dir.join(format!("{name}.png")), &png).unwrap();
        }
        let sky = CustomSky::load(&root);
        (root, sky)
    }

    fn noon(biome: &str) -> Conditions<'_> {
        Conditions {
            day_time: 6000,
            weather: Weather::Clear,
            biome,
            y: 64,
        }
    }

    #[test]
    fn layers_ease_in_at_a_biome_border_but_not_on_arrival() {
        let (root, sky) = pack(&[
            ("sky1", "biomes=desert\ntransition=2"),
            ("sky2", "blend=alpha\nrotate=false"),
        ]);
        let _ = std::fs::remove_dir_all(root);
        assert!(sky.has(0) && !sky.has(1));
        assert_eq!(sky.images[0].width, 6);
        let (mut state, mut out) = (State::default(), Vec::new());
        sky.draws(
            0,
            &noon("minecraft:desert"),
            0.0,
            0.05,
            &mut state,
            &mut out,
        );
        assert_eq!(
            out.iter().map(|d| d.brightness).collect::<Vec<_>>(),
            [1.0, 1.0]
        );
        // Out of the desert: the first layer fades over its two seconds.
        sky.draws(0, &noon("minecraft:plains"), 0.0, 0.5, &mut state, &mut out);
        assert_eq!(out[0].brightness, 0.75);
        assert_eq!(out[1].blend, engine::Blend::Alpha);
        sky.draws(0, &noon("minecraft:plains"), 0.0, 2.0, &mut state, &mut out);
        assert_eq!(out.len(), 1, "a dark layer is not drawn");
        assert_eq!(out[0].image, 1);
        // Into the End, which has no layers, and back: no fade.
        sky.draws(1, &noon("minecraft:desert"), 0.0, 0.0, &mut state, &mut out);
        assert!(out.is_empty());
        sky.draws(0, &noon("minecraft:desert"), 0.0, 0.0, &mut state, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn a_layer_turns_west_like_the_sun() {
        let (root, sky) = pack(&[("sky1", "")]);
        let _ = std::fs::remove_dir_all(root);
        let (mut state, mut out) = (State::default(), Vec::new());
        // A quarter turn after noon (sunset): the skybox's zenith has gone down to the west.
        let c = Conditions {
            day_time: 12000,
            ..noon("minecraft:plains")
        };
        sky.draws(0, &c, 0.25, 0.0, &mut state, &mut out);
        let up = out[0].rotation.transform_vector3(Vec3::Y);
        assert!((up - Vec3::NEG_X).length() < 1e-5, "{up}");
    }
}
