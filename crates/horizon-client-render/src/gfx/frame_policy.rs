//! The frame policy: which frame a renderer draws, what the overlays draw on and whether
//! Horizon's tone curve runs (tests in `frame_policy_tests.rs`).

use super::{LINEAR_FRAME, WIDE_FRAME};

/// What a renderer's frame is drawn in: `LINEAR_FRAME` under the switch where the device draws
/// it, for a 0.2 pack, or for the layered HDR frame where the surface offers HDR output
/// (`linear`: `(a 0.2 pack picked, the layered HDR frame, the device draws it)`); else
/// `WIDE_FRAME` when it is made with HDR on and Smooth Gradients (`wide_frame`,
/// `output::resolve_frame`), the surface offering HDR output (`hdr_offered`) and the device
/// drawing it (`draws_wide_frame`): an 8-bit frame's steps band in dark gradients, and HDR
/// shows them. Else the swapchain's 8-bit format, as without HDR.
pub(super) fn frame_format(
    wide_frame: bool,
    hdr_offered: bool,
    device_draws_it: bool,
    swapchain: wgpu::TextureFormat,
    linear: (bool, bool, bool),
) -> wgpu::TextureFormat {
    if linear.2 && (linear.0 || linear.1 && hdr_offered) {
        LINEAR_FRAME
    } else if wide_frame && hdr_offered && device_draws_it {
        WIDE_FRAME
    } else {
        swapchain.remove_srgb_suffix()
    }
}

/// The frame's format, and whether it is the layered HDR frame (HUD coverage): for HDR on
/// under the switch (`hdr_float`), with no pack or a 0.2 one, where the device keeps the HUD
/// on a layer of its own.
pub(super) fn frame_configuration(
    wide_frame: bool,
    device: (bool, bool, bool, bool),
    swapchain: wgpu::TextureFormat,
    linear_pack: bool,
    hdr_float: bool,
) -> (wgpu::TextureFormat, bool) {
    let (hdr_offered, wide_ok, linear_ok, coverage_ok) = device;
    let layered = hdr_float && hdr_offered && linear_ok && coverage_ok;
    (
        frame_format(
            wide_frame,
            hdr_offered,
            wide_ok,
            swapchain,
            (linear_pack, layered, linear_ok),
        ),
        layered,
    )
}

/// What the hand, the camera overlays and the HUD draw on: the frame, except for a 0.2 pack's
/// float frame without the HUD's layer, which is SDR: the pack's picture finishes on the
/// swapchain's 8-bit format first, so they clip and blend per draw as on vanilla's frame.
pub(super) fn overlay_format(
    frame: wgpu::TextureFormat,
    layered: bool,
    swapchain: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    if frame == LINEAR_FRAME && !layered {
        swapchain.remove_srgb_suffix()
    } else {
        frame
    }
}

/// Whether Horizon's tone curve (`Hdr::tonemap`) runs on this frame: on the layered frame's
/// world, unless a 0.2 pack's `[tonemap]` already mapped it. One tone mapping, never two.
pub(super) fn horizon_tone_curve(layered: bool, world: bool, pack_tonemap: bool) -> bool {
    layered && world && !pack_tonemap
}
