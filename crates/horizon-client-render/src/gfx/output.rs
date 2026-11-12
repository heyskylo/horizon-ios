//! The output each renderer owns (docs/design/hdr-linear-light.md): what its destination
//! offers and its display reports, resolved with the settings and the pack into one snapshot
//! that the settings screens, calibration, F3 and the frame read. Nothing here is shared
//! between renderers. A split view or an offscreen renderer never changes what a window
//! offers or reports.
use super::hdr::Output;
use crate::graphics::{Graphics, HdrMode, Reported};

/// What a renderer's destination and device can draw and present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OutputCapabilities {
    /// The HDR output the destination presents: its surface's, or offscreen the one
    /// `HORIZON_HDR_OUTPUT` names (`hdr::offscreen_output`).
    pub hdr: Option<Output>,
    /// The device draws, blends and filters the 16-bit frame (Smooth Gradients).
    pub wide_frame: bool,
    /// The same for the float frame (`LINEAR_FRAME`).
    pub float_frame: bool,
    /// The device keeps the HUD on a layer of its own (`hud_coverage::supported`).
    pub hud_layer: bool,
    /// HDR output is held back: the last start on this window's graphics driver ended before
    /// HDR output showed a frame ([`held`]). The mode stays; the output is SDR.
    pub held: bool,
}

/// Whether HDR output is held back on a renderer whose window's graphics driver is `driver`
/// (`None` offscreen, which never holds it back: only a window writes the marker).
pub(crate) fn held(settings: &Graphics, driver: Option<&str>) -> bool {
    driver.is_some_and(|driver| settings.hdr_held(driver))
}

/// The graphics driver this window's HDR output starts on ([`driver_fingerprint`]), with the
/// system's version as crash reports name it (`crash::operating_system`), read once a run.
pub(crate) fn window_driver(info: &wgpu::AdapterInfo) -> String {
    static OS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    driver_fingerprint(info, OS.get_or_init(crate::crash::operating_system))
}

/// The graphics driver a window's HDR output starts on, in one line: the backend, the
/// adapter's vendor and device IDs and its name, the driver's name and its version, and the
/// system's version `os`. Another GPU, a driver update or a system update reads differently:
/// Metal names no driver, so on macOS the driver updates with the system.
pub fn driver_fingerprint(info: &wgpu::AdapterInfo, os: &str) -> String {
    let adapter = format!(
        "{:?} {:04x}:{:04x} {}",
        info.backend, info.vendor, info.device, info.name
    );
    [adapter.as_str(), &info.driver, &info.driver_info, os]
        .map(|field| {
            field
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .join("; ")
}

/// What the system last reported for a renderer's display: its luminance, and whether it
/// shows HDR (`None`: it doesn't say, or nobody asked, as with HDR Off and offscreen).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayReading {
    pub(crate) luminance: Option<Reported>,
    pub(crate) hdr: Option<bool>,
}
impl DisplayReading {
    /// The watch stopped (HDR Off asks the system nothing): no system report. The luminance
    /// it last reported stays, which the settings' Auto values show.
    pub(crate) fn stop_watching(&mut self) {
        self.hdr = None;
    }
}

/// What the picked pack asks of the frame (`picked_pack_shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameRequest {
    /// A shader API 0.2 pack is picked under `HORIZON_HDR_FRAME=float`: its frame is float.
    pub linear_pack: bool,
    /// That pack's `[frame] blend` is `"linear"`: its frame holds linear light where it is
    /// float (`linear_blend`).
    pub linear_blend: bool,
    /// Under `HORIZON_HDR_FRAME=float`, with no pack available or a 0.2 one: the frame is
    /// the layered float one wherever HDR output goes.
    pub hdr_float: bool,
}

/// What a renderer's frame is drawn in, and what the hand, overlays and HUD draw on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameConfiguration {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) overlay_format: wgpu::TextureFormat,
    /// The layered float HDR frame: an 8-bit HUD target and an R8 coverage target beside it.
    pub(crate) hud_layer: bool,
    /// The frame holds linear light, not vanilla's gamma (`linear_blend::stores_linear`).
    pub(crate) linear_light: bool,
}

/// The display a 0.2 pack's `Frame` describes (docs/api/shaders-reference-0.2.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DisplayValues {
    pub hdr: bool,
    /// The peak over paper white, at least 1.
    pub headroom: f32,
    pub paper_white: f32,
    pub peak: f32,
    pub black_level: f32,
}
impl DisplayValues {
    pub const SDR: Self = Self {
        hdr: false,
        headroom: 1.0,
        paper_white: 80.0,
        peak: 80.0,
        black_level: 0.0,
    };
    /// The values HDR output's last pass maps to: the headroom never below 1, and the black
    /// level never above paper white.
    fn hdr([paper_white, peak, black]: [f32; 3]) -> Self {
        let paper_white = paper_white.max(1.0);
        let headroom = (peak / paper_white).max(1.0);
        Self {
            hdr: true,
            headroom,
            paper_white,
            peak: paper_white * headroom,
            black_level: black.clamp(0.0, paper_white),
        }
    }
}

/// One renderer's output as applied, with what its destination offers and reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectiveOutput {
    /// The HDR output this renderer's destination offers.
    pub(crate) offered: Option<Output>,
    /// What this renderer's display last reported (`DisplayReading`).
    pub(crate) system_hdr: Option<bool>,
    pub(crate) reported: Option<Reported>,
    /// The device draws the 16-bit frame (Smooth Gradients).
    pub(crate) wide_frame_supported: bool,
    /// HDR output is held back on this window's graphics driver (`OutputCapabilities::held`).
    pub(crate) held: bool,
    /// The HDR output presented; `None` is SDR.
    pub(crate) encoding: Option<Output>,
    pub(crate) frame: FrameConfiguration,
    /// Paper white, peak and black level in nits, as HDR output's last pass and its
    /// metadata take them, while it is on.
    pub(crate) nits: [f32; 3],
    /// The HUD and menus' brightness, as a share of paper white: their nits over its.
    pub(crate) ui_brightness: f32,
    /// The running shader pack lights the world itself: Horizon lifts none of it, so HDR
    /// Lighting has nothing to switch (`with_pack`).
    pub(crate) pack_lights: bool,
}
impl Default for EffectiveOutput {
    fn default() -> Self {
        Self::NONE
    }
}
impl EffectiveOutput {
    /// No destination yet: nothing offered or reported, SDR on an 8-bit frame.
    pub(crate) const NONE: Self = Self {
        offered: None,
        system_hdr: None,
        reported: None,
        wide_frame_supported: false,
        held: false,
        encoding: None,
        frame: FrameConfiguration {
            format: wgpu::TextureFormat::Bgra8Unorm,
            overlay_format: wgpu::TextureFormat::Bgra8Unorm,
            hud_layer: false,
            linear_light: false,
        },
        nits: [203.0, 1000.0, 0.0],
        ui_brightness: 1.0,
        pack_lights: false,
    };
    /// This output under the running shader pack: a `shaders/` one (`shaders`), or one of
    /// Horizon's own of API `native`. Under a 0.1 pack on the 8- or 16-bit frame the last pass
    /// still lifts the light sources and the sun its marks hold (`lifts_world` in
    /// `Renderer::render`); under the rest Horizon lifts nothing.
    pub(crate) fn with_pack(self, shaders: bool, native: Option<horizon_shader_api::Api>) -> Self {
        let pack_lights = shaders
            || native.is_some_and(|api| {
                api != horizon_shader_api::Api::V0_1 || self.frame.format == super::LINEAR_FRAME
            });
        Self {
            pack_lights,
            ..self
        }
    }
    /// Whether the destination offers HDR output.
    pub fn hdr_offered(&self) -> bool {
        self.offered.is_some()
    }
    /// Whether HDR output is presented.
    pub fn hdr_on(&self) -> bool {
        self.encoding.is_some()
    }
    /// Whether `mode` asks for HDR here: On always, Auto where the destination offers it and
    /// the display reports HDR, neither while HDR output is held back. The output still needs
    /// an offer and a frame that can take it.
    pub fn requests_hdr(&self, mode: HdrMode) -> bool {
        !self.held && requests_hdr(mode, self.offered.is_some(), self.system_hdr)
    }
    /// What a 0.2 pack's `Frame` describes: the HDR display on the layered frame, whose
    /// picture is light; SDR otherwise, where the frame holds SDR's picture, the one HDR
    /// output's last pass expands.
    pub(crate) fn display(&self) -> DisplayValues {
        if self.encoding.is_some() && self.frame.hud_layer {
            DisplayValues::hdr(self.nits)
        } else {
            DisplayValues::SDR
        }
    }
    /// What a scaled screenshot's frame is drawn for (`Renderer::draw_scaled_frame`): the
    /// layered frame keeps HDR, whose world light and HUD layer make the screenshot, so a 0.2
    /// pack reads this display as every other frame does; any other frame is drawn without
    /// HDR, into a target of its own, SDR.
    pub(crate) fn scaled_screenshot(self) -> Self {
        Self {
            encoding: self.encoding.filter(|_| self.frame.hud_layer),
            ..self
        }
    }
}

fn requests_hdr(mode: HdrMode, offered: bool, system: Option<bool>) -> bool {
    match mode {
        HdrMode::Off => false,
        HdrMode::On => true,
        HdrMode::Auto => offered && system == Some(true),
    }
}
/// Whether `settings` ask for HDR on a destination of `caps` whose display reports `reading`.
fn wanted(settings: &Graphics, caps: OutputCapabilities, reading: DisplayReading) -> bool {
    !caps.held && requests_hdr(settings.hdr, caps.hdr.is_some(), reading.hdr)
}

/// The frame a renderer made now draws in, for `settings`, the pack's `request`, what the
/// destination and device can do (`caps`) and what the display reports.
pub(crate) fn resolve_frame(
    settings: &Graphics,
    request: FrameRequest,
    caps: OutputCapabilities,
    reading: DisplayReading,
    swapchain: wgpu::TextureFormat,
) -> FrameConfiguration {
    let wanted = wanted(settings, caps, reading);
    let (format, hud_layer) = super::frame_configuration(
        wanted && settings.hdr_smooth_gradients,
        (
            caps.hdr.is_some(),
            caps.wide_frame,
            caps.float_frame,
            caps.hud_layer,
        ),
        swapchain,
        request.linear_pack,
        request.hdr_float && wanted,
    );
    FrameConfiguration {
        format,
        overlay_format: super::overlay_format(format, hud_layer, swapchain),
        hud_layer,
        linear_light: super::linear_blend::stores_linear(request.linear_blend, format),
    }
}

/// The output `settings` ask for on `frame`, the frame the renderer has: HDR output where the
/// mode asks for it, the destination offers it, presents (`presents`: a surface, or an
/// offscreen target), and the overlays draw on the frame (a 0.2 pack's float frame without
/// the HUD's layer finishes on 8 bits first, so it is SDR).
pub(crate) fn resolve_output(
    settings: &Graphics,
    caps: OutputCapabilities,
    reading: DisplayReading,
    frame: FrameConfiguration,
    presents: bool,
) -> EffectiveOutput {
    let wanted = wanted(settings, caps, reading);
    EffectiveOutput {
        offered: caps.hdr,
        system_hdr: reading.hdr,
        reported: reading.luminance,
        wide_frame_supported: caps.wide_frame,
        held: caps.held,
        encoding: caps
            .hdr
            .filter(|_| wanted && presents && frame.overlay_format == frame.format),
        frame,
        nits: [
            settings.paper_white_nits(reading.luminance) as f32,
            settings.peak_nits(reading.luminance) as f32,
            settings.black_level_nits(),
        ],
        ui_brightness: settings.ui_nits(reading.luminance) as f32
            / settings.paper_white_nits(reading.luminance) as f32,
        pack_lights: false,
    }
}

/// Whether HDR output lifts the world's light-source marks above paper white. Not on the title
/// screen or the float frame, and not under a pack that lights its world itself: a 0.2 pack is
/// shown at paper white, and a `shaders/` pack's `final` pass leaves its own alpha in the
/// frame, which isn't marks (HDR lighting is off under it too).
pub(crate) fn lifts_world(
    title: bool,
    float_frame: bool,
    shaders_pack: bool,
    native_api: Option<horizon_shader_api::Api>,
) -> bool {
    !title
        && !float_frame
        && !shaders_pack
        && native_api.is_none_or(|api| api == horizon_shader_api::Api::V0_1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::gfx::{LINEAR_FRAME, WIDE_FRAME};
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb};

    /// A window whose display offers `hdr` and reports `system` HDR at 400/1000 nits, on a
    /// device that draws every frame.
    pub(crate) fn window(
        hdr: Option<Output>,
        system: Option<bool>,
    ) -> (OutputCapabilities, DisplayReading) {
        (
            OutputCapabilities {
                hdr,
                wide_frame: true,
                float_frame: true,
                hud_layer: true,
                held: false,
            },
            DisplayReading {
                luminance: Some(Reported {
                    paper_white: 400,
                    peak: 1000,
                    peak_guessed: false,
                }),
                hdr: system,
            },
        )
    }
    /// The snapshot a renderer of `caps` and `reading` applies for `settings`, on the frame it
    /// would be made with.
    pub(crate) fn snapshot(
        settings: &Graphics,
        request: FrameRequest,
        caps: OutputCapabilities,
        reading: DisplayReading,
    ) -> EffectiveOutput {
        let frame = resolve_frame(settings, request, caps, reading, Bgra8UnormSrgb);
        resolve_output(settings, caps, reading, frame, true)
    }

    /// HDR output lifts light-source marks only on the 8/16-bit frame's world with no pack or
    /// a 0.1 pack: a `shaders/` pack's alpha isn't marks, and a 0.2 pack is shown at paper white.
    #[test]
    fn only_horizons_own_gamma_world_is_lifted() {
        use horizon_shader_api::Api::{V0_1, V0_2};
        // (title, float frame, `shaders/` pack, Horizon pack) -> lifts
        let cases = [
            ((false, false, false, None), true),
            ((false, false, false, Some(V0_1)), true),
            ((false, false, true, None), false),
            ((false, false, false, Some(V0_2)), false),
            ((false, true, false, None), false),
            ((true, false, false, None), false),
        ];
        for ((title, float, shaders, api), lifts) in cases {
            assert_eq!(
                lifts_world(title, float, shaders, api),
                lifts,
                "title {title}, float {float}, shaders/ pack {shaders}, pack {api:?}"
            );
        }
    }

    /// Every mode against every offer, system report, pack and device: HDR output only where
    /// the mode asks for it and the destination offers it, and the frame each combination
    /// draws in, as `Renderer::new` picked it before the snapshot.
    #[test]
    fn the_resolver_picks_one_output_and_frame_for_each_combination() {
        let no_pack = FrameRequest {
            linear_pack: false,
            linear_blend: false,
            hdr_float: true,
        };
        let pack_0_2 = FrameRequest {
            linear_pack: true,
            linear_blend: false,
            hdr_float: true,
        };
        // A 0.1 pack, or none without the float switch; a refused, missing or disabled
        // pick asks as no pack does (`picked_pack_shape`).
        let legacy = FrameRequest::default();
        for mode in HdrMode::ALL {
            for hdr in [None, Some(Output::Pq), Some(Output::ScRgb)] {
                for system in [None, Some(false), Some(true)] {
                    for (request, name) in
                        [(no_pack, "no pack"), (pack_0_2, "0.2"), (legacy, "0.1")]
                    {
                        for (float_frame, hud_layer) in
                            [(true, true), (true, false), (false, true), (false, false)]
                        {
                            for smooth in [false, true] {
                                let settings = Graphics {
                                    hdr: mode,
                                    hdr_smooth_gradients: smooth,
                                    ..Graphics::default()
                                };
                                let (mut caps, reading) = window(hdr, system);
                                (caps.float_frame, caps.hud_layer) = (float_frame, hud_layer);
                                let out = snapshot(&settings, request, caps, reading);
                                let wanted = match mode {
                                    HdrMode::Off => false,
                                    HdrMode::On => true,
                                    HdrMode::Auto => hdr.is_some() && system == Some(true),
                                };
                                let layered = request.hdr_float
                                    && wanted
                                    && hdr.is_some()
                                    && float_frame
                                    && hud_layer;
                                let format = if float_frame && (request.linear_pack || layered) {
                                    LINEAR_FRAME
                                } else if wanted && smooth && hdr.is_some() {
                                    WIDE_FRAME
                                } else {
                                    Bgra8Unorm
                                };
                                let case = format!(
                                    "{mode:?} {hdr:?} {system:?} {name} float {float_frame} \
                                     layer {hud_layer} smooth {smooth}"
                                );
                                assert_eq!(out.frame.format, format, "{case}");
                                assert_eq!(out.frame.hud_layer, layered, "{case}");
                                // A 0.2 pack's float frame without the layer finishes on 8 bits.
                                let overlays = if format == LINEAR_FRAME && !layered {
                                    Bgra8Unorm
                                } else {
                                    format
                                };
                                assert_eq!(out.frame.overlay_format, overlays, "{case}");
                                let encoding = hdr.filter(|_| wanted && overlays == format);
                                assert_eq!(out.encoding, encoding, "{case}");
                                assert_eq!(out.requests_hdr(mode), wanted, "{case}");
                                assert_eq!((out.offered, out.system_hdr), (hdr, system), "{case}");
                                // A 0.2 pack sees the HDR display only on the layered frame.
                                assert_eq!(
                                    out.display().hdr,
                                    encoding.is_some() && layered,
                                    "{case}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Nothing is presented without a surface or an offscreen target, and the requested mode
    /// stays as it was: only the output follows.
    #[test]
    fn a_destination_that_presents_nothing_is_sdr() {
        let settings = Graphics {
            hdr: HdrMode::On,
            ..Graphics::default()
        };
        let (caps, reading) = window(Some(Output::Pq), Some(true));
        let frame = resolve_frame(
            &settings,
            FrameRequest::default(),
            caps,
            reading,
            Bgra8Unorm,
        );
        assert_eq!(
            resolve_output(&settings, caps, reading, frame, false).encoding,
            None
        );
        assert_eq!(
            resolve_output(&settings, caps, reading, frame, true).encoding,
            Some(Output::Pq)
        );
        assert_eq!(settings.hdr, HdrMode::On);
    }

    /// A window on the driver a start failed on stays SDR, on an SDR frame, whatever the
    /// mode, and the mode stays as chosen; another driver tries HDR again, and an offscreen
    /// renderer (no driver) never holds HDR back.
    #[test]
    fn a_failed_start_holds_hdr_back_on_its_driver_only() {
        let failed = "Vulkan 10de:2684 GPU; driver; 1.0";
        for mode in [HdrMode::On, HdrMode::Auto] {
            let settings = Graphics {
                hdr: mode,
                hdr_failed_start: Some(failed.into()),
                ..Graphics::default()
            };
            let request = FrameRequest {
                linear_pack: false,
                linear_blend: false,
                hdr_float: true,
            };
            let on = |driver: Option<&str>| {
                let (mut caps, reading) = window(Some(Output::Pq), Some(true));
                caps.held = held(&settings, driver);
                snapshot(&settings, request, caps, reading)
            };
            let out = on(Some(failed));
            assert_eq!(out.encoding, None, "{mode:?}");
            assert_eq!(out.frame.format, Bgra8Unorm, "{mode:?}");
            assert!(!out.frame.hud_layer && out.held && !out.requests_hdr(mode));
            // Still offered: the page lets the player pick a mode to try again.
            assert!(out.hdr_offered());
            assert_eq!(settings.hdr, mode);
            for driver in [Some("Vulkan 10de:2684 GPU; driver; 1.1"), None] {
                let out = on(driver);
                assert_eq!(out.encoding, Some(Output::Pq), "{mode:?} {driver:?}");
                assert_eq!(out.frame.format, LINEAR_FRAME, "{mode:?} {driver:?}");
                assert!(!out.held && out.requests_hdr(mode));
            }
        }
        // A marker that named no driver holds on whichever window comes first.
        let legacy = Graphics {
            hdr: HdrMode::On,
            hdr_failed_start: Some(String::new()),
            ..Graphics::default()
        };
        assert!(held(&legacy, Some(failed)));
        assert!(!held(&legacy, None));
    }

    /// The fingerprint tells another GPU, backend, driver version or system version apart, in
    /// one line.
    #[test]
    fn the_driver_fingerprint_is_one_line_that_changes_with_the_driver() {
        let os = "windows (x86_64) version Windows 11 (26200.7019)";
        let info = wgpu::AdapterInfo {
            name: "NVIDIA GeForce RTX 4090".into(),
            vendor: 0x10de,
            device: 0x2684,
            device_type: wgpu::DeviceType::DiscreteGpu,
            device_pci_bus_id: "0000:01:00.0".into(),
            driver: "NVIDIA".into(),
            driver_info: "581.57\n".into(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 32,
            subgroup_max_size: 32,
            transient_saves_memory: None,
            limit_bucket: None,
        };
        let fingerprint = driver_fingerprint(&info, os);
        assert_eq!(
            fingerprint,
            "Vulkan 10de:2684 NVIDIA GeForce RTX 4090; NVIDIA; 581.57; \
             windows (x86_64) version Windows 11 (26200.7019)"
        );
        assert_ne!(
            driver_fingerprint(&info, "windows (x86_64) version Windows 11 (26200.7171)"),
            fingerprint
        );
        // Metal names no driver: a macOS update is what changes it.
        let metal = wgpu::AdapterInfo {
            name: "Apple M3".into(),
            vendor: 0,
            device: 0,
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Metal,
            ..info.clone()
        };
        let sonoma = driver_fingerprint(&metal, "macos (aarch64) version macOS 14.6");
        assert_eq!(
            sonoma,
            "Metal 0000:0000 Apple M3; ; ; macos (aarch64) version macOS 14.6"
        );
        assert_ne!(
            driver_fingerprint(&metal, "macos (aarch64) version macOS 15.1"),
            sonoma
        );
        let other = [
            wgpu::AdapterInfo {
                driver_info: "582.02".into(),
                ..info.clone()
            },
            wgpu::AdapterInfo {
                backend: wgpu::Backend::Dx12,
                ..info.clone()
            },
            wgpu::AdapterInfo {
                device: 0x2704,
                ..info.clone()
            },
        ];
        for other in other {
            assert_ne!(driver_fingerprint(&other, os), fingerprint, "{other:?}");
        }
    }

    /// Auto's paper white and peak are this display's report; the player's own values win.
    #[test]
    fn auto_levels_follow_this_renderers_report() {
        let mut settings = Graphics {
            hdr: HdrMode::On,
            ..Graphics::default()
        };
        let (caps, reading) = window(Some(Output::Pq), Some(true));
        let frame = resolve_frame(
            &settings,
            FrameRequest::default(),
            caps,
            reading,
            Bgra8Unorm,
        );
        let out = resolve_output(&settings, caps, reading, frame, true);
        assert_eq!(out.nits, [400.0, 1000.0, 0.0]);
        let unreported = DisplayReading::default();
        assert_eq!(
            resolve_output(&settings, caps, unreported, frame, true).nits,
            [203.0, 1000.0, 0.0]
        );
        (settings.hdr_paper_white, settings.hdr_peak) = (250, 1500);
        assert_eq!(
            resolve_output(&settings, caps, reading, frame, true).nits,
            [250.0, 1500.0, 0.0]
        );
    }

    /// UI Brightness reaches HDR output as its nits over paper white's: 1 on Auto, whatever
    /// paper white is; a value set by hand holds its nits, Paper White on Auto or set by hand.
    #[test]
    fn ui_brightness_is_its_nits_over_paper_whites() {
        let (caps, reading) = window(Some(Output::Pq), Some(true));
        let share = |paper_white, ui_nits, reading| {
            let settings = Graphics {
                hdr: HdrMode::On,
                hdr_paper_white: paper_white,
                hdr_ui_nits: ui_nits,
                ..Graphics::default()
            };
            let frame = resolve_frame(
                &settings,
                FrameRequest::default(),
                caps,
                reading,
                Bgra8Unorm,
            );
            resolve_output(&settings, caps, reading, frame, true).ui_brightness
        };
        // Auto: the report's 400 nits, 203 without one, or the player's 250.
        let unreported = DisplayReading::default();
        for (paper_white, reading) in [(0, reading), (0, unreported), (250, reading)] {
            assert_eq!(share(paper_white, 0, reading), 1.0);
        }
        assert_eq!(share(0, 100, reading), 0.25);
        assert_eq!(share(0, 480, reading), 1.2);
        assert_eq!(share(0, 406, unreported), 2.0);
        assert_eq!(share(250, 100, reading), 0.4);
        assert_eq!(share(250, 480, reading), 1.92);
    }

    /// Two renderers' states, a window's and an offscreen one's, made and changed in turn:
    /// neither's offer or report shows in the other's snapshot, and the offscreen one
    /// stopping its watch (HDR Off) leaves the window's report alone.
    #[test]
    fn two_renderers_keep_their_own_offer_and_report() {
        let auto = Graphics {
            hdr: HdrMode::Auto,
            ..Graphics::default()
        };
        let (window_caps, mut window_reading) = window(Some(Output::Pq), Some(true));
        let window_frame = resolve_frame(
            &auto,
            FrameRequest::default(),
            window_caps,
            window_reading,
            Bgra8Unorm,
        );
        let window_output =
            |reading| resolve_output(&auto, window_caps, reading, window_frame, true);
        let before = window_output(window_reading);
        assert_eq!(before.encoding, Some(Output::Pq));
        // The offscreen renderer is made after: it offers nothing and nothing reported.
        let (offscreen_caps, _) = window(None, None);
        let mut offscreen_reading = DisplayReading::default();
        let offscreen_frame = resolve_frame(
            &auto,
            FrameRequest::default(),
            offscreen_caps,
            offscreen_reading,
            Bgra8Unorm,
        );
        let offscreen = resolve_output(
            &auto,
            offscreen_caps,
            offscreen_reading,
            offscreen_frame,
            true,
        );
        assert_eq!(
            (offscreen.offered, offscreen.reported, offscreen.encoding),
            (None, None, None)
        );
        assert_eq!(window_output(window_reading), before);
        // The offscreen renderer's watch stops; the window's report stays.
        offscreen_reading.stop_watching();
        assert_eq!(window_output(window_reading), before);
        // The window's display changes its report; the offscreen renderer still has none.
        window_reading.luminance = Some(Reported {
            paper_white: 300,
            peak: 800,
            peak_guessed: false,
        });
        assert_eq!(window_output(window_reading).nits[..2], [300.0, 800.0]);
        assert_eq!(
            resolve_output(
                &auto,
                offscreen_caps,
                offscreen_reading,
                offscreen_frame,
                true
            )
            .reported,
            None
        );
        // The window's own watch stopping clears its system report, not its luminance (the
        // settings' Auto values keep showing what the display last said).
        window_reading.stop_watching();
        let stopped = window_output(window_reading);
        assert_eq!((stopped.system_hdr, stopped.encoding), (None, None));
        assert_eq!(stopped.reported, window_reading.luminance);
    }

    /// A scaled screenshot of the layered frame keeps HDR, and a 0.2 pack reads the window's
    /// display in it, not SDR's; the 16-bit and the 8-bit frames shoot SDR, as do SDR windows.
    #[test]
    fn a_scaled_screenshot_keeps_the_display_only_on_the_layered_frame() {
        let on = Graphics {
            hdr: HdrMode::On,
            ..Graphics::default()
        };
        let no_pack = FrameRequest {
            linear_pack: false,
            linear_blend: false,
            hdr_float: true,
        };
        let (caps, reading) = window(Some(Output::Pq), Some(true));
        let layered = snapshot(&on, no_pack, caps, reading);
        assert!(layered.frame.hud_layer && layered.display().hdr);
        let shot = layered.scaled_screenshot();
        assert_eq!(shot.encoding, Some(Output::Pq));
        assert_eq!(shot.display(), layered.display());
        for out in [
            snapshot(&on, FrameRequest::default(), caps, reading),
            snapshot(&Graphics::default(), no_pack, caps, reading),
        ] {
            let shot = out.scaled_screenshot();
            assert_eq!((shot.encoding, shot.display()), (None, DisplayValues::SDR));
            assert_eq!(shot.frame, out.frame);
        }
    }

    /// What a 0.2 pack reads of the display: the headroom the tone curve eases into, never
    /// below 1, and the black Horizon lifts after it, never above paper white.
    #[test]
    fn the_display_a_pack_reads_is_the_one_the_curve_maps_to() {
        let d = DisplayValues::hdr([203.0, 1000.0, 0.05]);
        assert_eq!(
            (d.headroom, d.paper_white, d.black_level),
            (1000.0 / 203.0, 203.0, 0.05)
        );
        assert!((d.peak - 1000.0).abs() < 1e-3, "{}", d.peak);
        // A peak under paper white has no headroom: the display tops out at white.
        let d = DisplayValues::hdr([203.0, 150.0, 400.0]);
        assert_eq!(
            (d.headroom, d.paper_white, d.peak, d.black_level),
            (1.0, 203.0, 203.0, 203.0)
        );
    }
}
