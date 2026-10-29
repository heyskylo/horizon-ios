//! Native extras remain available where the Wasm runtime is disabled.
use super::*;
pub const ZOOM_KEY: &str = "horizon.zoom";
pub const ZOOM_LEVELS: [u8; 6] = [2, 3, 4, 6, 8, 10];
pub const ZOOM_MIN: f32 = 1.25;
pub const ZOOM_MAX: f32 = 10.0;
const ZOOM_STEP: f32 = 1.25;
/// The zoom key's state: held or not, and the zoom the wheel has set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    held: bool,
    factor: f32,
}
impl Default for Zoom {
    fn default() -> Self {
        Self {
            held: false,
            factor: 1.0,
        }
    }
}
impl Zoom {
    /// The key went down: start at the default level.
    pub fn press(&mut self, level: u8) {
        self.held = true;
        self.factor = f32::from(level).clamp(ZOOM_MIN, ZOOM_MAX);
    }
    pub fn release(&mut self) {
        self.held = false;
    }
    /// A press or release of the zoom key. Only a press while `playing` (no
    /// screen or text field open) zooms; anywhere else the key belongs to
    /// the screen, and the zoom lets go.
    pub fn key(&mut self, pressed: bool, repeat: bool, playing: bool, level: u8) {
        if !pressed || !playing {
            self.release();
        } else if !repeat {
            self.press(level);
        }
    }
    pub fn held(&self) -> bool {
        self.held
    }
    /// Wheel steps while held (positive zooms in); false when not zooming.
    pub fn scroll(&mut self, steps: f32) -> bool {
        if !self.held {
            return false;
        }
        self.factor = (self.factor * ZOOM_STEP.powf(steps)).clamp(ZOOM_MIN, ZOOM_MAX);
        true
    }
    /// The zoom in effect: 1 when the key is up or the spyglass is in use
    /// (scoping keeps vanilla's spyglass view).
    pub fn factor(&self, scoping: bool) -> f32 {
        if self.held && !scoping {
            self.factor
        } else {
            1.0
        }
    }
}
/// Vanilla key mappings bound to the zoom key. The key still reaches them
/// (a key bound twice triggers both, as vanilla's do), so the Extras page
/// says so.
pub fn zoom_key_conflicts(e: &Extras, keys: &crate::keybinds::KeyBinds) -> Vec<&'static str> {
    if e.zoom_key == "key.keyboard.unknown" {
        return Vec::new();
    }
    crate::keybinds::MAPPINGS
        .iter()
        .map(|m| m.0)
        .filter(|name| keys.get(name) == e.zoom_key)
        .collect()
}
/// The mouse turning scale while zoomed.
pub fn turn_scale(zoom: f32, lower_sensitivity: bool) -> f64 {
    if lower_sensitivity && zoom > 1.0 {
        1.0 / f64::from(zoom)
    } else {
        1.0
    }
}
