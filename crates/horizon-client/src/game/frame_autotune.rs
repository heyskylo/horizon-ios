//! What the settings' automatic modes decide from each rendered frame: GPU Culling's Auto
//! (`autotune`) and Dynamic Resolution (`dynamic_resolution`).

use horizon_client::autotune::GpuCullingAuto;
use horizon_client::dynamic_resolution::{DynamicResolution, Pace};
use horizon_client::gfx::Renderer;
use horizon_client::graphics::{GpuCullingMode, Graphics};
use horizon_client::perf_overlay::FrameGraph;

/// One rendered frame of `r`, its CPU and whole time in milliseconds. `settled`: the player is
/// in a world that has loaded, the only frames either mode counts (a chunk load's slow frames
/// would read as a slow scene).
pub(super) fn observe(
    g: &Graphics,
    settled: bool,
    (culling, dynamic): (&mut GpuCullingAuto, &mut DynamicResolution),
    graph: &mut FrameGraph,
    r: &mut Renderer,
    (cpu_ms, frame_ms): (f32, f32),
) {
    // The performance overlay names the scale while it moves on its own.
    graph.dynamic_scale = g
        .dynamic_resolution
        .then(|| dynamic.scale(g.resolution_scale));
    if !settled {
        return;
    }
    let gpu_ms = r.gpu_times.last().copied();
    // GPU Culling's Auto, only where it can be switched.
    if g.gpu_culling == GpuCullingMode::Auto
        && horizon_client::graphics::gpu_culling_offer()
            .reason()
            .is_none()
        && let Some(on) = culling.observe(cpu_ms, gpu_ms, frame_ms)
    {
        log::info!(
            "Horizon: GPU Culling Auto turns it {} ({})",
            if on { "on" } else { "off" },
            culling.state()
        );
        if r.set_gpu_culling(on).is_err() {
            culling.reset();
        }
    }
    // Dynamic Resolution: gentle while a shader pack runs (its history starts over at each
    // change), quick for Horizon's own renderer.
    if g.dynamic_resolution {
        let pace = if r.shader_pack_kind().is_some() {
            Pace::Gentle
        } else {
            Pace::Quick
        };
        if let Some(scale) =
            dynamic.observe(gpu_ms, frame_ms, g.resolution_scale, g.target_fps, pace)
        {
            // Debug only: it changes as often as the scene does, which would flood the log.
            log::debug!("Horizon: Dynamic Resolution sets the world to {scale}%");
            apply_world_post(r, g, scale);
        }
    }
}

/// The world's resolution scale in percent: Dynamic Resolution's where it is on, else the
/// setting's.
pub(super) fn world_scale(g: &Graphics, dynamic: &DynamicResolution) -> u32 {
    if g.dynamic_resolution {
        dynamic.scale(g.resolution_scale)
    } else {
        g.resolution_scale
    }
}

/// The world's post-processing settings, at the resolution scale `scale` (percent).
pub(super) fn apply_world_post(r: &mut Renderer, g: &Graphics, scale: u32) {
    r.set_world_post(
        scale as f32 / 100.0,
        g.fxaa,
        g.taa,
        g.upscaling.ratio(),
        g.upscaling_sharpness as f32 / 100.0,
    );
}
