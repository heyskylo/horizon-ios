//! A shader API 0.2 pack's `[frame] blend = "linear"` (docs/design/hdr-linear-light.md, How the
//! frame stores the picture): its float frame holds linear light instead of vanilla's gamma, so
//! glass, water and smoke blend as light does. The pack's material, sky and weather stages store
//! their results as they are. Horizon's own draws into the frame decode theirs at the end of
//! their shaders (hdr_light.wgsl's `frame_stored`), so a solid surface looks as under `"gamma"`.
//! What the pack reads of the frame (`scene_color`, `opaque_color`, the water mirror) is copied
//! as it is instead of decoded. The tonemap encodes the picture into gamma again, so the hand,
//! the camera overlays, the HUD, the screenshots and HDR output see the same either way.

/// Whether a frame of `format` stores linear light for a 0.2 pack that asks for it (`linear`,
/// `FrameRequest::linear_blend`): only the float frame. Where the device can't draw that, the
/// 0.2 pack runs on the 8-bit frame, which keeps vanilla's gamma, as `"gamma"` does: eight bits
/// of linear light would band in the darks.
pub(super) fn stores_linear(linear: bool, format: wgpu::TextureFormat) -> bool {
    linear && format == super::LINEAR_FRAME
}

/// The constants of a pipeline of Horizon's that draws into the world's frame of `format`
/// (hdr_light.wgsl): light written, not marked, on the float frame; and decoded where the
/// frame holds linear light (`linear`).
pub(super) fn frame_constants(
    format: wgpu::TextureFormat,
    linear: bool,
) -> [(&'static str, f64); 2] {
    [
        (
            "HDR_WRITE_LIGHT",
            f64::from(u8::from(format == super::LINEAR_FRAME)),
        ),
        ("FRAME_LINEAR", f64::from(u8::from(linear))),
    ]
}

/// The world's clear colour, given in vanilla's gamma, as a frame that holds linear light
/// (`linear`) stores it.
pub(super) fn stored_clear(color: wgpu::Color, linear: bool) -> wgpu::Color {
    if !linear {
        return color;
    }
    let decode = |v: f64| v.max(0.0).powf(2.2);
    wgpu::Color {
        r: decode(color.r),
        g: decode(color.g),
        b: decode(color.b),
        a: color.a,
    }
}

#[cfg(test)]
#[path = "linear_blend_tests.rs"]
mod tests;
