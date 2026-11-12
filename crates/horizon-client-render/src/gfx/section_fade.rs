//! How far through vanilla's chunk fade-in a section is, and the instance that carries it to `terrain.wgsl`.
use crate::fast_hash::FxHashMap;
use std::time::Instant;

/// A section's instance: its camera-relative origin and its UV step, negated
/// past its fade code while it fades in over far terrain (`lod::instance`).
pub(super) fn section_instance(origin: [f32; 3], fade: u32, visibility: f32) -> [f32; 4] {
    let step = 1.0 / crate::mesher::UV_STEPS;
    [
        origin[0],
        origin[1],
        origin[2],
        if fade == 0 {
            // The step is a power of two, so its mantissa is free: it holds the vanilla
            // chunk fade-in's `1 - visibility` in 23 bits (terrain.wgsl's `instance_step` and
            // `instance_visibility` read it back). A whole section is the bare step.
            let faded = ((1.0 - visibility) * 8_388_608.0)
                .round()
                .clamp(0.0, 8_388_607.0) as u32;
            f32::from_bits(step.to_bits() | faded)
        } else {
            -(fade as f32 + step)
        },
    ]
}
/// `SectionRenderDispatcher.RenderSection.getVisibility`: how far through its fade-in a section
/// uploaded `elapsed_ms` ago is.
pub(super) fn chunk_visibility(elapsed_ms: u64, fade_ms: u64) -> f32 {
    if elapsed_ms >= fade_ms {
        1.0
    } else {
        elapsed_ms as f32 / fade_ms as f32
    }
}
/// `Util.toMillis`: `Mth.floor(seconds * 1000.0)`.
pub(super) fn fade_millis(seconds: f32) -> u64 {
    (f64::from(seconds) * 1000.0).floor().max(0.0) as u64
}
/// `chunk_visibility` at `now` of a section first uploaded at `uploaded`
/// (`Renderer::section_uploaded`; none: one that fades in over far terrain, or
/// whose column left), with the chunk fade-in `fade_ms`.
pub(super) fn section_visibility(uploaded: Option<&Instant>, fade_ms: u64, now: Instant) -> f32 {
    uploaded.map_or(1.0, |&t| {
        let elapsed = now.saturating_duration_since(t).as_millis() as u64;
        chunk_visibility(elapsed, fade_ms)
    })
}
/// What a section's fade depends on besides the section itself
/// (`SectionGpu::drawn_faded`): the time, the chunk fade-in time, the sections'
/// first uploads (`Renderer::section_uploaded`) and those held while far
/// terrain comes in (`Renderer::lod_retiring`).
#[derive(Clone, Copy)]
pub(super) struct FadeInputs<'a> {
    pub(super) now: Instant,
    pub(super) fade_ms: u64,
    pub(super) uploaded: &'a FxHashMap<super::SectionKey, Instant>,
    pub(super) retiring: &'a crate::fast_hash::FxHashSet<super::SectionKey>,
}
/// `LevelRenderer.CHUNK_VISIBILITY_THRESHOLD`: a section's block entities wait for it.
pub(super) const CHUNK_VISIBILITY_THRESHOLD: f32 = 0.3;
