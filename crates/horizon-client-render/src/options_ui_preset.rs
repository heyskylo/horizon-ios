//! The graphics preset: `Options.graphicsPreset`'s slider position and the
//! options `GraphicsPreset.apply` sets.
use super::*;

pub(crate) const PRESETS: [&str; 4] = ["fast", "fancy", "fabulous", "custom"];
/// `SliderableEnum.fromSliderValue`: the slider's 0..1 position maps by
/// `floor(v * values)` with 1.0 just below it, clamped to the last stop (4
/// graphics presets), not by rounding `v * 3`.
pub(crate) fn preset_from_slider(v: f32) -> usize {
    let v = if v >= 1.0 { 0.99999 } else { v };
    ((v * 4.0).floor() as usize).min(3)
}
pub(crate) fn preset_index(p: &str) -> usize {
    PRESETS.iter().position(|x| *x == p).unwrap_or(1)
}
/// A slider moved to `v` (OptionInstance.set): the graphics preset applies
/// its options when its value changes (`Options.applyGraphicsPreset`).
pub(crate) fn set_slider(opt: Opt, s: &mut Settings, v: f32) {
    let preset = preset_index(&s.graphics_preset);
    opt.set_slider(s, v);
    if opt == Opt::Preset && preset_index(&s.graphics_preset) != preset {
        apply_preset(s);
    }
}
/// A slider pressed or dragged to `v`: apply it live, and tell the game when
/// a category volume changed (`Options.createSoundSliderOptionInstance`'s
/// callback, which `OptionInstance.set` runs only for a new value).
pub(crate) fn drag_slider(opt: Opt, s: &mut Settings, v: f32, events: &mut Vec<Event>) {
    let volume = |s: &Settings| match opt {
        Opt::Volume(0) => Some(s.master_volume),
        Opt::Volume(i) => Some(s.volumes[i]),
        _ => None,
    };
    let before = volume(s);
    set_slider(opt, s, v);
    events.push(Event::SliderDrag);
    if let (Opt::Volume(source), Some(after)) = (opt, volume(s))
        && before != Some(after)
    {
        events.push(Event::Volume(source, after));
    }
}
/// GraphicsPreset.apply: the options Fast, Fancy and Fabulous set.
pub(crate) fn apply_preset(s: &mut Settings) {
    let fabulous = s.graphics_preset == "fabulous";
    match s.graphics_preset.as_str() {
        "fast" => {
            s.biome_blend = 1;
            s.render_distance = 8;
            s.prioritize_chunk_updates = 0;
            s.simulation_distance = 6;
            s.smooth_lighting = false;
            s.clouds = "fast".into();
            s.particles = 1;
            s.mipmap_levels = 2;
            s.entity_shadows = false;
            s.entity_distance = 0.75;
            s.menu_blur = 2;
            s.cloud_range = 32;
            s.cutout_leaves = false;
            s.improved_transparency = false;
            s.weather_radius = 5;
            set_filtering(s, 1, 0);
        }
        "fancy" | "fabulous" => {
            s.biome_blend = 2;
            s.render_distance = if fabulous { 32 } else { 16 };
            s.prioritize_chunk_updates = 1;
            s.simulation_distance = 12;
            s.smooth_lighting = true;
            s.clouds = "true".into();
            s.particles = 0;
            s.mipmap_levels = 4;
            s.entity_shadows = true;
            s.entity_distance = if fabulous { 1.25 } else { 1.0 };
            s.menu_blur = 5;
            s.cloud_range = if fabulous { 128 } else { 64 };
            s.cutout_leaves = true;
            s.improved_transparency = fabulous;
            s.weather_radius = 10;
            // Fabulous takes anisotropic filtering (RGSS on a device with known issues, which
            // wgpu does not report).
            set_filtering(
                s,
                if fabulous { 2 } else { 1 },
                if fabulous { 2 } else { 1 },
            );
        }
        _ => {}
    }
}
/// The preset's `maxAnisotropyBit` and `textureFiltering` (0 none, 1 RGSS, 2 anisotropic).
fn set_filtering(s: &mut Settings, anisotropy_bit: i32, filtering: i32) {
    use crate::options_file::V;
    s.file.set("maxAnisotropyBit", V::Int(anisotropy_bit));
    s.file.set("textureFiltering", V::Int(filtering));
}
