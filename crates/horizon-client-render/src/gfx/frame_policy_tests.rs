//! The frame policy: which frame a renderer draws (`frame_configuration`), when it must be made
//! again (`frame_follows`), and what follows from the frame chosen.

#[test]
fn float_frames_never_schedule_light_mark_passes() {
    use super::{LINEAR_FRAME, WIDE_FRAME, light_marks};
    use wgpu::TextureFormat::{Bgra8Unorm, Rgb10a2Unorm, Rgba8Unorm};
    for hdr in [false, true] {
        assert!(!light_marks(LINEAR_FRAME, hdr));
        for format in [WIDE_FRAME, Bgra8Unorm, Rgba8Unorm, Rgb10a2Unorm] {
            assert_eq!(light_marks(format, hdr), hdr);
        }
    }
}

#[test]
fn frame_policy_keeps_compatibility_frames_and_requires_layering_for_float_hdr() {
    use super::{LINEAR_FRAME, WIDE_FRAME, frame_configuration};
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb};

    for bits in 0..128 {
        let hdr = bits & 1 != 0;
        let offered = bits & 2 != 0;
        let wide_ok = bits & 4 != 0;
        let float_ok = bits & 8 != 0;
        let coverage_ok = bits & 16 != 0;
        let switch = bits & 32 != 0;
        let smooth = bits & 64 != 0;
        for pack in ["none", "0.1", "0.2", "shaders"] {
            let float_hdr = matches!(pack, "none" | "0.2");
            let expected = if switch && float_ok && float_hdr && hdr && offered && coverage_ok {
                (LINEAR_FRAME, true)
            } else if switch && float_ok && pack == "0.2" {
                (LINEAR_FRAME, false)
            } else if hdr && smooth && offered && wide_ok {
                (WIDE_FRAME, false)
            } else {
                (Bgra8Unorm, false)
            };
            assert_eq!(
                frame_configuration(
                    hdr && smooth,
                    (offered, wide_ok, float_ok, coverage_ok),
                    Bgra8UnormSrgb,
                    switch && pack == "0.2",
                    switch && hdr && float_hdr,
                ),
                expected,
                "bits {bits}, pack {pack}"
            );
        }
    }
}

#[test]
fn float_hdr_falls_back_for_each_missing_hud_layer_requirement() {
    use super::{LINEAR_FRAME, WIDE_FRAME, frame_configuration, hud_coverage};
    use wgpu::{DownlevelFlags as D, TextureFormat as F, TextureFormatFeatureFlags as Fl};
    use wgpu::{TextureFormatFeatures, TextureUsages as U};

    let usages = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING;
    for (attachments, flags, allowed_usages, format_flags) in [
        (1, D::INDEPENDENT_BLEND, usages, Fl::BLENDABLE),
        (2, D::empty(), usages, Fl::BLENDABLE),
        (2, D::INDEPENDENT_BLEND, U::TEXTURE_BINDING, Fl::BLENDABLE),
        (2, D::INDEPENDENT_BLEND, U::RENDER_ATTACHMENT, Fl::BLENDABLE),
        (2, D::INDEPENDENT_BLEND, usages, Fl::empty()),
    ] {
        let coverage = hud_coverage::supported(
            attachments,
            flags,
            TextureFormatFeatures {
                allowed_usages,
                flags: format_flags,
            },
        );
        let device = (true, true, true, coverage);
        assert_eq!(
            frame_configuration(true, device, F::Bgra8Unorm, false, true),
            (WIDE_FRAME, false)
        );
        assert_eq!(
            frame_configuration(false, device, F::Bgra8Unorm, false, true),
            (F::Bgra8Unorm, false)
        );
        assert_eq!(
            frame_configuration(true, device, F::Bgra8Unorm, true, false),
            (LINEAR_FRAME, false)
        );
        // A 0.2 pack keeps its float frame, without the layer: SDR.
        assert_eq!(
            frame_configuration(true, device, F::Bgra8Unorm, true, true),
            (LINEAR_FRAME, false)
        );
    }
}

#[test]
fn a_0_2_pack_without_the_hud_layer_finishes_on_the_swapchains_8_bits() {
    use super::{LINEAR_FRAME, WIDE_FRAME, overlay_format};
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm};
    assert_eq!(
        overlay_format(LINEAR_FRAME, false, Bgra8UnormSrgb),
        Bgra8Unorm
    );
    assert_eq!(overlay_format(LINEAR_FRAME, false, Rgba8Unorm), Rgba8Unorm);
    // Everything else draws its overlays on its frame: the layered frame's light, the
    // 16-bit frame, the 8-bit frame.
    for (frame, layered) in [
        (LINEAR_FRAME, true),
        (WIDE_FRAME, false),
        (Bgra8Unorm, false),
    ] {
        assert_eq!(overlay_format(frame, layered, Bgra8UnormSrgb), frame);
    }
}

#[test]
fn one_tone_curve_runs_on_the_layered_world() {
    use super::horizon_tone_curve;
    // No pack, or a 0.2 pack without `[tonemap]`: Horizon's curve.
    assert!(horizon_tone_curve(true, true, false));
    // A pack's `[tonemap]` mapped it already; never both.
    assert!(!horizon_tone_curve(true, true, true));
    // No world (the title screen), or no layered frame (SDR, the compatibility frames).
    assert!(!horizon_tone_curve(true, false, false));
    assert!(!horizon_tone_curve(false, true, false));
}

#[test]
fn frame_follows_requires_a_rebuild_when_only_layering_changes() {
    use super::{LINEAR_FRAME, frame_follows};
    use wgpu::TextureFormat::Bgra8Unorm;

    let device = (true, true, true, true);
    assert!(frame_follows(
        (LINEAR_FRAME, true),
        true,
        device,
        Bgra8Unorm,
        false,
        true
    ));
    assert!(!frame_follows(
        (LINEAR_FRAME, true),
        true,
        device,
        Bgra8Unorm,
        true,
        false
    ));
    assert!(frame_follows(
        (LINEAR_FRAME, false),
        true,
        device,
        Bgra8Unorm,
        true,
        false
    ));
    // HDR turned on under a 0.2 pack: the layered frame.
    assert!(!frame_follows(
        (LINEAR_FRAME, false),
        true,
        device,
        Bgra8Unorm,
        true,
        true
    ));
    assert!(frame_follows(
        (LINEAR_FRAME, true),
        true,
        device,
        Bgra8Unorm,
        true,
        true
    ));
    assert!(!frame_follows(
        (LINEAR_FRAME, false),
        true,
        device,
        Bgra8Unorm,
        false,
        true
    ));
}

#[test]
fn hdr_availability_refresh_rebuilds_a_frame_created_in_sdr() {
    use super::{LINEAR_FRAME, frame_follows, hdr::Output, update_hdr_output};
    use wgpu::TextureFormat::Bgra8Unorm;

    let mut output = None;
    let mut device = (false, true, true, true);
    let sdr = (Bgra8Unorm, false);
    let follows = |frame, device, on| frame_follows(frame, on, device, Bgra8Unorm, false, on);
    assert!(follows(sdr, device, false));
    update_hdr_output(&mut output, &mut device, Some(Output::Pq));
    assert!(!follows(sdr, device, true));
    assert!(follows((LINEAR_FRAME, true), device, true));
}

#[test]
fn only_a_no_pack_layered_world_applies_hdr_gain() {
    use super::no_pack_hdr_gain;
    assert!(no_pack_hdr_gain(true, false, true));
    assert!(!no_pack_hdr_gain(false, false, true));
    assert!(!no_pack_hdr_gain(true, true, true));
    assert!(!no_pack_hdr_gain(true, false, false));
}
