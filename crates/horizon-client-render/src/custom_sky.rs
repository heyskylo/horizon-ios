//! Custom sky layers: skyboxes a pack draws over the vanilla sky, in a format vanilla doesn't read
//! (OptiFine's), which the platform's pack formats fill ([`crate::pack_hooks`]). The layers'
//! textures are read once with the pack; each frame the game asks for the layers to draw, their
//! brightness and rotation ([`SkyLayers::draws`]), and `gfx::custom_sky` draws them over the
//! vanilla sky. A pack without layers keeps the vanilla sky.

use std::path::Path;
use std::sync::Arc;

use glam::Mat4;

/// A layer's texture, RGBA8: the skybox's six faces in a 3x2 grid, bottom, top and south in the
/// top row, west, north and east below (`gfx::custom_sky`'s faces).
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How a layer mixes with what is drawn below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Add,
    Subtract,
    Multiply,
    Dodge,
    Burn,
    Screen,
    Replace,
    Overlay,
    Alpha,
}

/// One layer of a frame: which texture, with which blend, how bright and turned how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub image: u16,
    pub blend: Blend,
    pub brightness: f32,
    /// Camera-relative world from the skybox's space.
    pub rotation: Mat4,
}

/// The weather of a frame: thunder is a storm's rain, so it is only `Thunder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Clear,
    Rain,
    Thunder,
}

/// What a frame's layers may depend on.
#[derive(Clone, Copy, Debug)]
pub struct Conditions<'a> {
    /// Ticks of the day, counting the days.
    pub day_time: i64,
    pub weather: Weather,
    /// The biome at the camera, namespaced.
    pub biome: &'a str,
    /// The camera's block height.
    pub y: i32,
}

/// The eased brightness of each layer, kept by the game between frames.
#[derive(Default)]
pub struct State {
    /// The world the layers were last drawn for.
    pub world: Option<i32>,
    /// Each layer's eased visibility, 0 to 1.
    pub eased: Vec<f32>,
}

/// A pack's custom sky layers, by world (`0` the Overworld, `1` the End).
pub trait SkyLayers: Send + Sync {
    /// Every layer's texture: [`Draw::image`] indexes it.
    fn images(&self) -> &[Image];
    /// Whether `world` has layers.
    fn has(&self, world: i32) -> bool;
    /// This frame's layers of `world` into `out` (cleared first), in drawing order, eased by
    /// `seconds` since the last frame. `sun_turns` is the sun's angle over 360° (`0` at noon).
    fn draws(
        &self,
        world: i32,
        conditions: &Conditions,
        sun_turns: f32,
        seconds: f32,
        state: &mut State,
        out: &mut Vec<Draw>,
    );
}

/// No layers: the vanilla sky.
struct NoLayers;

impl SkyLayers for NoLayers {
    fn images(&self) -> &[Image] {
        &[]
    }

    fn has(&self, _: i32) -> bool {
        false
    }

    fn draws(&self, _: i32, _: &Conditions, _: f32, _: f32, s: &mut State, out: &mut Vec<Draw>) {
        out.clear();
        s.world = None;
    }
}

/// The custom sky layers of the overlay at `root`, from the installed pack formats.
pub fn load(root: &Path) -> Arc<dyn SkyLayers> {
    crate::pack_hooks::formats()
        .custom_sky(root)
        .unwrap_or_else(|| Arc::new(NoLayers))
}
