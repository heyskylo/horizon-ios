//! Horizon's own graphics settings, beyond vanilla's options: kept in
//! `horizon-graphics.txt` next to `options.txt` so that file stays exactly
//! vanilla's. Every default reproduces vanilla's rendering; only choices the
//! player makes here depart from it.
use std::{fs, path::Path};

pub const FILE: &str = "horizon-graphics.txt";
/// Written beside it before a window's HDR output is switched on, naming the graphics driver it
/// starts on ([`hdr_marker_text`]), and removed once the first HDR frame is presented. Found at
/// the next start, it means HDR output took the game down there: HDR output is held back on
/// that driver ([`Graphics::hdr_failed_start`]), and the mode stays as the player chose it.
pub const HDR_MARKER: &str = "horizon-hdr-pending";
/// The marker's text for a window whose HDR output starts on `driver`
/// (`gfx::output::window_driver`).
pub fn hdr_marker_text(driver: &str) -> String {
    format!("driver:{driver}\n")
}
/// The driver a marker names; empty for one written before markers named it.
fn marker_driver(text: &str) -> &str {
    text.lines()
        .find_map(|line| line.strip_prefix("driver:"))
        .unwrap_or_default()
        .trim()
}

/// What the system reports for the display HDR output goes to, in nits in HDR output's
/// scale: where its SDR white sits (the desktop's white) and the peak. Each renderer keeps its
/// own display's (`gfx::DisplayReading`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reported {
    pub paper_white: u32,
    pub peak: u32,
}

/// The top of Peak Brightness's scale: `peak_nits_at(PEAK_STEPS)` is PQ's 10 000 nits.
pub const PEAK_STEPS: u32 = 52;
/// Peak Brightness's value at `step` of its scale, the slider's and calibration's: 200 to
/// 2000 nits by 50, where most displays' peaks are, then up to PQ's 10 000 by 500.
pub fn peak_nits_at(step: u32) -> u32 {
    match step.min(PEAK_STEPS) {
        s @ 0..=36 => 200 + s * 50,
        s => 2000 + (s - 36) * 500,
    }
}
/// Where `nits` sits on Peak Brightness's scale ([`peak_nits_at`]), between steps for a value
/// off it.
pub fn peak_step_of(nits: f32) -> f32 {
    let step = if nits <= 2000.0 {
        (nits - 200.0) / 50.0
    } else {
        36.0 + (nits - 2000.0) / 500.0
    };
    step.clamp(0.0, PEAK_STEPS as f32)
}

/// Simple-level presets. `Custom` leaves every option as the player set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Preset {
    Performance,
    Balanced,
    Quality,
    Ultra,
    #[default]
    Custom,
}
impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Performance,
        Preset::Balanced,
        Preset::Quality,
        Preset::Ultra,
        Preset::Custom,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Preset::Performance => "performance",
            Preset::Balanced => "balanced",
            Preset::Quality => "quality",
            Preset::Ultra => "ultra",
            Preset::Custom => "custom",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Preset::Performance => "Performance",
            Preset::Balanced => "Balanced",
            Preset::Quality => "Quality",
            Preset::Ultra => "Ultra",
            Preset::Custom => "Custom",
        }
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .unwrap_or_default()
    }
}

/// Entity occlusion strategy. The synchronous mode preserves the original
/// same-frame CPU test; asynchronous results only hide a mesh when their
/// camera, bounds and occluder evidence still match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EntityCullingMode {
    #[default]
    Off,
    Synchronous,
    Asynchronous,
}
impl EntityCullingMode {
    pub const ALL: [EntityCullingMode; 3] = [
        EntityCullingMode::Off,
        EntityCullingMode::Synchronous,
        EntityCullingMode::Asynchronous,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Synchronous => "sync",
            Self::Asynchronous => "async",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Synchronous => "Synchronous",
            Self::Asynchronous => "Asynchronous",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            // Keep graphics files written before the mode selector readable.
            "true" | "synchronous" | "sync" => Self::Synchronous,
            "false" | "off" => Self::Off,
            "asynchronous" | "async" => Self::Asynchronous,
            _ => Self::default(),
        }
    }
}

/// Particle visibility strategy, independent of Entity Culling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ParticleCullingMode {
    #[default]
    Off,
    Synchronous,
    Asynchronous,
}
impl ParticleCullingMode {
    pub const ALL: [ParticleCullingMode; 3] = [
        ParticleCullingMode::Off,
        ParticleCullingMode::Synchronous,
        ParticleCullingMode::Asynchronous,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Synchronous => "sync",
            Self::Asynchronous => "async",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Synchronous => "Synchronous",
            Self::Asynchronous => "Asynchronous",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "synchronous" | "sync" => Self::Synchronous,
            "asynchronous" | "async" => Self::Asynchronous,
            "off" => Self::Off,
            _ => Self::default(),
        }
    }
}

/// How the swapchain presents frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PresentMode {
    /// Vanilla's choice: Fifo with VSync on, else the first of Immediate,
    /// Mailbox and Fifo the surface supports. The other modes apply only
    /// with VSync off: VSync on always presents with Fifo.
    #[default]
    Auto,
    Fifo,
    Mailbox,
    Immediate,
}
impl PresentMode {
    pub const ALL: [PresentMode; 4] = [
        PresentMode::Auto,
        PresentMode::Fifo,
        PresentMode::Mailbox,
        PresentMode::Immediate,
    ];
    pub fn name(self) -> &'static str {
        match self {
            PresentMode::Auto => "auto",
            PresentMode::Fifo => "fifo",
            PresentMode::Mailbox => "mailbox",
            PresentMode::Immediate => "immediate",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PresentMode::Auto => "Auto (VSync)",
            PresentMode::Fifo => "FIFO",
            PresentMode::Mailbox => "Mailbox",
            PresentMode::Immediate => "Immediate",
        }
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .unwrap_or_default()
    }
}

/// The graphics API the renderer runs on. Windows picks Vulkan, DirectX 12 or
/// OpenGL; Linux picks Vulkan or OpenGL; macOS always uses its one backend
/// (Metal). Applies at the next start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GraphicsApi {
    /// Vulkan, then DirectX 12 (Windows), then OpenGL when neither has a surface, adapter or device.
    #[default]
    Auto,
    Vulkan,
    Dx12,
    /// OpenGL, for GPUs with no Vulkan or DirectX 12 driver (Windows and Linux).
    OpenGl,
}
impl GraphicsApi {
    pub const ALL: [GraphicsApi; 4] = [
        GraphicsApi::Auto,
        GraphicsApi::Vulkan,
        GraphicsApi::Dx12,
        GraphicsApi::OpenGl,
    ];
    /// The APIs this system offers, in the order the setting cycles through
    /// them: DirectX 12 is Windows' third backend, and macOS has no choice.
    pub fn choices() -> &'static [GraphicsApi] {
        const WINDOWS: [GraphicsApi; 4] = [
            GraphicsApi::Auto,
            GraphicsApi::Vulkan,
            GraphicsApi::Dx12,
            GraphicsApi::OpenGl,
        ];
        const LINUX: [GraphicsApi; 3] =
            [GraphicsApi::Auto, GraphicsApi::Vulkan, GraphicsApi::OpenGl];
        const MACOS: [GraphicsApi; 1] = [GraphicsApi::Auto];
        if cfg!(windows) {
            &WINDOWS
        } else if cfg!(target_os = "macos") {
            &MACOS
        } else {
            &LINUX
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            GraphicsApi::Auto => "auto",
            GraphicsApi::Vulkan => "vulkan",
            GraphicsApi::Dx12 => "dx12",
            GraphicsApi::OpenGl => "opengl",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GraphicsApi::Auto => "Auto",
            GraphicsApi::Vulkan => "Vulkan",
            GraphicsApi::Dx12 => "DirectX 12",
            GraphicsApi::OpenGl => "OpenGL",
        }
    }
    /// The API a name stands for, ignoring case; `None` for anything else.
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|api| api.name().eq_ignore_ascii_case(s.trim()))
    }
    /// `HORIZON_GRAPHICS_API` (auto, vulkan, dx12 or opengl) if set, else the setting.
    pub fn with_env(self) -> Self {
        std::env::var("HORIZON_GRAPHICS_API")
            .ok()
            .and_then(|v| Self::from_name(&v))
            .unwrap_or(self)
    }
}

/// What the frame limiter does in the last half millisecond before a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pacing {
    /// Sleep the whole wait: no CPU spent spinning, a little less precise; frames
    /// keep to a fixed grid of slots, so late wake-ups don't lower the frame rate.
    #[default]
    Efficient,
    /// Vanilla's `FramerateLimiter`: sleep, then spin until the deadline.
    Precise,
}

/// The performance overlay drawn over the HUD.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overlay {
    #[default]
    Off,
    /// Frames per second and frame time.
    Fps,
    /// The numbers and a graph of recent frame times.
    Graph,
}

/// How finely far terrain is drawn (`FarDetail` in the LOD options).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FarDetail {
    /// Each tile level starts at eight times its size from the camera.
    #[default]
    Full,
    /// Coarser levels start at half that distance: about a quarter of the
    /// far quads, for GPUs that far terrain slows down. Changes the picture.
    Reduced,
}
impl FarDetail {
    pub fn name(self) -> &'static str {
        match self {
            FarDetail::Full => "full",
            FarDetail::Reduced => "reduced",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FarDetail::Full => "Full",
            FarDetail::Reduced => "Reduced",
        }
    }
    fn parse(s: &str) -> Self {
        if s == "reduced" {
            FarDetail::Reduced
        } else {
            FarDetail::Full
        }
    }
}

/// Frame generation: a frame made between each two real ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FrameGeneration {
    #[default]
    Off,
    /// AMD FidelityFX Super Resolution 3.1's frame interpolation, ported to WGSL.
    Fsr31,
}
impl FrameGeneration {
    pub const ALL: [FrameGeneration; 2] = [FrameGeneration::Off, FrameGeneration::Fsr31];
    pub fn name(self) -> &'static str {
        match self {
            FrameGeneration::Off => "off",
            FrameGeneration::Fsr31 => "fsr3.1",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FrameGeneration::Off => "Off",
            FrameGeneration::Fsr31 => "FSR 3.1",
        }
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.name() == s)
            .unwrap_or_default()
    }
}

/// What the section mesher does with leaf faces inside a canopy. Terrain
/// geometry only: entities, particles and block entities are untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeafCullingMode {
    /// Every leaf face, as vanilla's fancy leaves draw them.
    #[default]
    Off,
    /// Drops a face between two leaves when the leaf it faces has a leaf or
    /// a full opaque cube behind it too: that face only shows through two
    /// cutout layers. The faces behind the outer layer stay, so a crown
    /// looks the same from outside. The faced leaf must be in the section
    /// (as `Hollow`), so the block behind it is at most a border cell of the
    /// next section, whose edits remesh this one.
    Hidden,
    /// Leaves surrounded on all six sides by leaves or full opaque cubes draw
    /// in the opaque layer, so their cutouts no longer show the layers behind.
    Solid,
    /// Like `Solid`, but a leaf with one open side draws opaque too: the
    /// exposed shell thins by a layer and the canopy looks chunkier.
    SolidAggressive,
    /// Drops the faces two leaves share. Faces toward air, cavities, partial
    /// models, unknown neighbours and section boundaries stay.
    Hollow,
}
impl LeafCullingMode {
    pub const ALL: [LeafCullingMode; 5] = [
        LeafCullingMode::Off,
        LeafCullingMode::Hidden,
        LeafCullingMode::Solid,
        LeafCullingMode::SolidAggressive,
        LeafCullingMode::Hollow,
    ];
    pub fn name(self) -> &'static str {
        match self {
            LeafCullingMode::Off => "off",
            LeafCullingMode::Hidden => "hidden",
            LeafCullingMode::Solid => "solid",
            LeafCullingMode::SolidAggressive => "solid_aggressive",
            LeafCullingMode::Hollow => "hollow",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LeafCullingMode::Off => "Off",
            LeafCullingMode::Hidden => "Hidden Faces",
            LeafCullingMode::Solid => "Solid",
            LeafCullingMode::SolidAggressive => "Solid Aggressive",
            LeafCullingMode::Hollow => "Hollow",
        }
    }
    /// Whether this mode draws interior leaf faces in the solid layer.
    pub fn is_interior(self) -> bool {
        matches!(
            self,
            LeafCullingMode::Solid | LeafCullingMode::SolidAggressive
        )
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == s)
            .unwrap_or_default()
    }
}

/// TAA upscaling: the world renders this much smaller than the resolution scale's size,
/// and TAA builds the full-size picture from the jittered frames (FSR's and DLSS's ratios).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Upscaling {
    #[default]
    Native,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}
impl Upscaling {
    pub const ALL: [Upscaling; 5] = [
        Upscaling::Native,
        Upscaling::Quality,
        Upscaling::Balanced,
        Upscaling::Performance,
        Upscaling::UltraPerformance,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Upscaling::Native => "native",
            Upscaling::Quality => "quality",
            Upscaling::Balanced => "balanced",
            Upscaling::Performance => "performance",
            Upscaling::UltraPerformance => "ultraPerformance",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Upscaling::Native => "Native",
            Upscaling::Quality => "Quality",
            Upscaling::Balanced => "Balanced",
            Upscaling::Performance => "Performance",
            Upscaling::UltraPerformance => "Ultra Performance",
        }
    }
    /// Output pixels per rendered pixel, along each axis.
    pub fn ratio(self) -> f32 {
        match self {
            Upscaling::Native => 1.0,
            Upscaling::Quality => 1.5,
            Upscaling::Balanced => 1.7,
            Upscaling::Performance => 2.0,
            Upscaling::UltraPerformance => 3.0,
        }
    }
    fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|u| u.name() == s)
            .unwrap_or_default()
    }
}

static REDUCED_SUGGESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the settings suggest Reduced far detail: set when the renderer
/// starts, on the GPUs `lod_capped` names (far terrain slows them most).
pub fn suggest_reduced_far_detail(on: bool) {
    REDUCED_SUGGESTED.store(on, std::sync::atomic::Ordering::Relaxed);
}
pub fn reduced_far_detail_suggested() -> bool {
    REDUCED_SUGGESTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The name the options screen waits on to bind `Graphics::hdr_key`.
pub const HDR_KEY: &str = "horizon.hdrSettings";
/// The Screenshot Quality choices, times the game's resolution.
pub const SCREENSHOT_QUALITIES: [u32; 4] = [1, 2, 4, 8];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HdrMode {
    #[default]
    Off,
    On,
    Auto,
}
impl HdrMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::On, Self::Auto];
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "false",
            Self::On => "true",
            Self::Auto => "auto",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::Auto => "Auto",
        }
    }
    fn parse(value: &str, default: Self) -> Self {
        match value {
            "false" | "off" => Self::Off,
            "true" | "on" => Self::On,
            "auto" => Self::Auto,
            _ => default,
        }
    }
}

static TRANSLUCENT_BATCHING_OFFERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Whether the device can batch translucent quads (`Graphics::translucent_batching`): set when
/// the renderer starts; not without indirect multi-draw or on OpenGL, which loops over a
/// multi-draw's entries on the CPU, nor where vanilla frames draw flat (they read the section
/// arena's translucent quads). Where it can't, the settings grey its switch out.
pub fn offer_translucent_batching(on: bool) {
    TRANSLUCENT_BATCHING_OFFERED.store(on, std::sync::atomic::Ordering::Relaxed);
}
pub fn translucent_batching_offered() -> bool {
    TRANSLUCENT_BATCHING_OFFERED.load(std::sync::atomic::Ordering::Relaxed)
}

/// GPU Culling's setting: off, on where it is offered, or Auto, which has it on only where the
/// CPU limits the frame and it makes the frame faster (`autotune::GpuCullingAuto`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GpuCullingMode {
    #[default]
    Off,
    On,
    Auto,
}
impl GpuCullingMode {
    pub const ALL: [GpuCullingMode; 3] = [
        GpuCullingMode::Off,
        GpuCullingMode::On,
        GpuCullingMode::Auto,
    ];
    /// The value in `horizon-graphics.txt`: `true` and `false` as the switch wrote them.
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "false",
            Self::On => "true",
            Self::Auto => "auto",
        }
    }
    /// The word `horizon-ctl gpu-culling` takes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Auto => "auto",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::Auto => "Auto",
        }
    }
    fn parse(s: &str, default: Self) -> Self {
        match s {
            "true" | "on" => Self::On,
            "false" | "off" => Self::Off,
            "auto" => Self::Auto,
            _ => default,
        }
    }
}

/// Whether GPU Culling (`Graphics::gpu_culling`) can be switched on here, and why not: set when a
/// renderer starts, and again when its pass can't be set up or its buffers stop fitting in video
/// memory. Where it isn't offered the settings grey its switch out with the reason, and the saved
/// value waits for a renderer that offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuCullingOffer {
    /// No count draws the GPU wrote, compute shaders or working first instances (OpenGL, Metal,
    /// Vulkan without `VK_KHR_draw_indirect_count`, Intel Gen11/Gen12's Vulkan driver), vanilla
    /// frames drawn flat, or a pass the driver couldn't build.
    Unsupported = 0,
    /// A software adapter (WARP, lavapipe): the pass would run on the CPU, beside the frame.
    Software = 1,
    /// Its buffers stopped fitting in video memory: off until the next renderer.
    OutOfMemory = 2,
    /// DX12 with flat page groups, whose count draws leave each draw's base vertex out of
    /// what the near sections' packed faces are read by (`gpu_culling::loses_base_vertex`).
    Dx12 = 3,
    /// The graphics device was lost while its frames were drawn (`lose_gpu_culling`): off
    /// for the renderers after it, until Horizon restarts.
    DeviceLost = 5,
    Offered = 4,
}
impl GpuCullingOffer {
    /// The greyed switch's tooltip; `None` where it is offered.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Unsupported => Some("Your GPU or graphics API doesn't support it."),
            Self::Software => Some("Not with software rendering, which runs it on the CPU."),
            Self::OutOfMemory => Some("Out of video memory: off until the next start."),
            Self::Dx12 => Some("Not on DirectX 12 yet: try the Vulkan graphics API."),
            Self::DeviceLost => Some("The graphics device was lost: off until the next start."),
            Self::Offered => None,
        }
    }
}
static GPU_CULLING_OFFER: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(GpuCullingOffer::Unsupported as u8);
pub fn offer_gpu_culling(offer: GpuCullingOffer) {
    GPU_CULLING_OFFER.store(offer as u8, std::sync::atomic::Ordering::Relaxed);
}
pub fn gpu_culling_offer() -> GpuCullingOffer {
    match GPU_CULLING_OFFER.load(std::sync::atomic::Ordering::Relaxed) {
        1 => GpuCullingOffer::Software,
        2 => GpuCullingOffer::OutOfMemory,
        3 => GpuCullingOffer::Dx12,
        4 => GpuCullingOffer::Offered,
        5 => GpuCullingOffer::DeviceLost,
        _ => GpuCullingOffer::Unsupported,
    }
}
/// Set once the graphics device was lost while GPU culling drew the frames (the game's
/// `recover_device`): the renderers made after it don't offer it (`GpuCullingOffer::DeviceLost`),
/// so a fault in its frames can't lose the device again after every recovery.
static GPU_CULLING_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn lose_gpu_culling() {
    GPU_CULLING_LOST.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub fn gpu_culling_lost() -> bool {
    GPU_CULLING_LOST.load(std::sync::atomic::Ordering::Relaxed)
}

/// The farthest LOD Distance, in chunks, on integrated GPUs other than
/// Apple's: they share the CPU's memory bandwidth, and past it far terrain
/// costs them more frame time than it is worth.
pub const INTEGRATED_LOD_DISTANCE: u32 = 128;
/// Whether `INTEGRATED_LOD_DISTANCE` applies to this adapter. Apple's GPUs
/// are integrated too, with memory and bandwidth enough for every distance.
pub fn lod_capped(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::IntegratedGpu && !info.name.starts_with("Apple")
}
/// A LOD distance as far terrain uses it on this adapter.
pub fn lod_distance_capped(distance: u32, info: &wgpu::AdapterInfo) -> u32 {
    let distance = distance.min(crate::lod::MAX_CHUNKS);
    if lod_capped(info) {
        distance.min(INTEGRATED_LOD_DISTANCE)
    } else {
        distance
    }
}
impl Graphics {
    /// `lod_distance` as far terrain uses it on this adapter.
    pub fn lod_distance_on(&self, info: &wgpu::AdapterInfo) -> u32 {
        lod_distance_capped(self.lod_distance, info)
    }
    /// Whether the selected packs leave vanilla's block models in place.
    /// Horizon's occlusion assumptions (entity and leaf culling) hold only
    /// for those: a pack can replace an opaque cube's model or a leaf's.
    pub fn vanilla_models(&self, resource_packs: &[String]) -> bool {
        resource_packs.iter().all(|id| id == "vanilla")
    }
    /// The same check including packs overlaid by the connected server.
    /// Their ids are namespaced as `server/...`, so an active server pack
    /// disables model-dependent occlusion assumptions.
    pub fn vanilla_models_with_server_packs<'a>(
        &self,
        resource_packs: impl IntoIterator<Item = &'a str>,
        server_packs: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        resource_packs
            .into_iter()
            .chain(server_packs)
            .all(|id| id == "vanilla")
    }
    /// The leaf culling the mesher may apply. A pack that can replace leaf
    /// models falls back to `Off`, which draws exactly vanilla's leaves.
    pub fn leaf_culling_effective(&self, resource_packs: &[String]) -> LeafCullingMode {
        if self.vanilla_models(resource_packs) {
            self.leaf_culling
        } else {
            LeafCullingMode::Off
        }
    }
    /// The effective mode when local and server resource packs are active.
    pub fn leaf_culling_effective_with_server_packs<'a>(
        &self,
        resource_packs: impl IntoIterator<Item = &'a str>,
        server_packs: impl IntoIterator<Item = &'a str>,
    ) -> LeafCullingMode {
        if self.vanilla_models_with_server_packs(resource_packs, server_packs) {
            self.leaf_culling
        } else {
            LeafCullingMode::Off
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Graphics {
    pub preset: Preset,
    /// Distant surface terrain range, in chunks; 0 disables LODs.
    pub lod_distance: u32,
    /// How finely far terrain is drawn; Full unless the player picks Reduced.
    pub far_detail: FarDetail,
    /// Far terrain from servers' BlueMap web maps; no server has one unless
    /// the player names it (`blueMap:` lines) or turns on auto-detect.
    pub blue_map: crate::bluemap::Settings,
    /// Skip sections hidden behind closed ones (vanilla's occlusion graph).
    pub cave_culling: bool,
    /// Near solid terrain culled, and its draws listed, on the GPU (`gfx::gpu_culling`), where it
    /// is offered (`gpu_culling_offer`). Off by default (GPU Culling, Performance graphics); it
    /// applies at once, and no preset sets it. Every save writes it, so a default turned on later
    /// needs a new key, as `batchTranslucent` did. Auto turns it on only where the CPU limits the
    /// frame and it makes the frame faster (`autotune`).
    pub gpu_culling: GpuCullingMode,
    /// Skip drawing entities hidden behind opaque terrain, synchronously or asynchronously.
    pub entity_culling: EntityCullingMode,
    /// Minimum entity population for coarse AABB culling; zero disables it.
    pub entity_culling_precheck: u32,
    /// Skip particles outside the view or hidden behind opaque terrain.
    pub particle_culling: ParticleCullingMode,
    /// Terrain leaf faces inside a canopy (see [`LeafCullingMode`]); `Off` draws
    /// vanilla's fancy leaves. Ignored with a resource pack that may replace
    /// the vanilla leaf models ([`Graphics::leaf_culling_effective`]).
    pub leaf_culling: LeafCullingMode,
    /// Draw regular ice opaque in the solid pass, without its see-through texture.
    pub opaque_ice: bool,
    /// Cache settled shulker-box models.
    pub cache_shulker_boxes: bool,
    /// Cache sign-text geometry.
    pub cache_signs: bool,
    /// Cache decorated pots outside their wobble animation.
    pub cache_decorated_pots: bool,
    /// Cache bells while they are not ringing.
    pub cache_bells: bool,
    /// Cache closed chests, including copper variants.
    pub cache_chests: bool,
    /// Cache banner poles while leaving their flags animated.
    pub cache_banners: bool,
    /// Cache copper-golem statue models.
    pub cache_copper_golem_statues: bool,
    /// Meshed sections uploaded per frame; 0 uploads all that are ready.
    pub chunk_updates: u32,
    /// Section mesh bytes uploaded per frame, in KiB; 0 is unlimited.
    pub upload_budget_kib: u32,
    /// The block textures' video memory (their top mip level), in MiB; 0 leaves it to
    /// Horizon (`sprites::DEFAULT_TEXTURE_BUDGET`). Applies when resources next load.
    pub texture_budget_mib: u32,
    pub sky: bool,
    pub fog: bool,
    pub weather: bool,
    pub animated_textures: bool,
    /// Dithers the small mip levels of cutouts from 64 texels up, so a high-resolution pack's
    /// fine plants keep their coverage in the distance; off is vanilla's look.
    pub dither_cutout_mips: bool,
    pub present_mode: PresentMode,
    pub pacing: Pacing,
    /// Chunk worker threads; 0 uses every core but one. Applies at the next start.
    pub workers: u32,
    pub overlay: Overlay,
    /// World resolution as a percentage of the window's (50..=200).
    pub resolution_scale: u32,
    /// FXAA over the world.
    pub fxaa: bool,
    /// TAA over the world (FXAA is off while it is on).
    pub taa: bool,
    /// TAA upscaling; anything but `Native` comes with `taa` on.
    pub upscaling: Upscaling,
    /// How much the upscaled picture is sharpened, 0..=100 (percent of the most).
    pub upscaling_sharpness: u32,
    /// F2 screenshots at this many times the game's resolution: 1, 2, 4 or 8.
    pub screenshot_quality: u32,
    /// Frame generation (Horizon setting), off by default.
    pub frame_generation: FrameGeneration,
    /// HDR output (HDR10, or scRGB), where the display path offers it.
    pub hdr: HdrMode,
    /// HDR paper white (where SDR white sits), in nits; 0 is Auto: the desktop's SDR
    /// white, as the system reports it ([`Graphics::paper_white_nits`]).
    pub hdr_paper_white: u32,
    /// HDR peak brightness the world's highlights reach, in nits (200..=10 000); 0 is Auto:
    /// what the system reports for the display ([`Graphics::peak_nits`]).
    pub hdr_peak: u32,
    /// HDR black level: the display's darkest black, in hundredths of a nit; 0, the default,
    /// lifts nothing. Shadows are lifted onto it, and the HDR metadata says it.
    pub hdr_black_level: u32,
    /// HDR brightness of the HUD and menus, in percent of paper white (25..=100); the
    /// first-person hand stays at the world's.
    pub hdr_ui_brightness: u32,
    /// HDR Lighting: Horizon's own HDR look, light sources, the sun and sunlight above paper
    /// white. Off, HDR output shows the same picture as SDR, for shader packs or the vanilla look.
    pub hdr_lighting: bool,
    /// A vanilla key name (`HDR_KEY`): opens HDR's settings from the game. Unbound by default.
    pub hdr_key: String,
    /// The graphics driver whose last start ended before HDR output showed a frame (the
    /// marker found, [`HDR_MARKER`]): a window on it stays SDR whatever the mode, until the
    /// mode is changed or the window starts on another driver ([`Graphics::hdr_held`]).
    /// Empty for a marker that named no driver: the first one seen holds it.
    pub hdr_failed_start: Option<String>,
    /// Battery mode: at most 60 frames per second in a world and 30 in menus
    /// or behind other windows, and half the worker threads. Off by default;
    /// it changes when frames are drawn, never what they show or the ticks.
    pub battery: bool,
    /// Reuse the shader pack's last shadow map when sun, view, scene revision and
    /// pack uniforms still match. Off by default: vanilla loaders redraw every frame
    /// (or every `shadowInterval`), and a hit with a stale key shows old shadows.
    pub shadow_cache: bool,
    /// The graphics API (Windows only; see `GraphicsApi`).
    pub api: GraphicsApi,
    /// Translucent quads in vertex buffers of their own (`gfx`'s translucent arena), so their
    /// back-to-front draws go out in a few calls, for a little more video memory. On by
    /// default (Translucent Batching, Advanced graphics); read when a renderer is made (start,
    /// resource-pack reload), where it is offered (`translucent_batching_offered`). Its key is
    /// `batchTranslucent`: `translucentBatching`, which every save wrote as false while it was
    /// off by default and had no switch, stays ignored.
    pub translucent_batching: bool,
    /// Smooth Gradients (HDR page, on by default): with HDR output on, the frame is drawn in
    /// 16 bits, so dark gradients don't band (`Renderer::frame_format`). Read when a renderer
    /// is made (start, resource-pack reload), like `translucent_batching`, where the renderer's
    /// device draws it (`EffectiveOutput::wide_frame_supported`).
    pub hdr_smooth_gradients: bool,
    /// The native `horizon:<manifest-id>` or classic filename picked in `shaderpacks/`; empty: none. Its key is
    /// not `shaderPack`, which versions before the clean-room rewrite saved and which stays
    /// ignored.
    pub shader_pack: String,
}
impl Default for Graphics {
    fn default() -> Self {
        Self {
            preset: Preset::Custom,
            lod_distance: 0,
            far_detail: FarDetail::Full,
            blue_map: crate::bluemap::Settings::default(),
            cave_culling: true,
            gpu_culling: GpuCullingMode::Off,
            entity_culling: EntityCullingMode::Off,
            entity_culling_precheck: 64,
            particle_culling: ParticleCullingMode::Off,
            leaf_culling: LeafCullingMode::Off,
            opaque_ice: false,
            cache_shulker_boxes: false,
            // Sign text has used its renderer cache independently of the new model caches.
            cache_signs: true,
            cache_decorated_pots: false,
            cache_bells: false,
            cache_chests: false,
            cache_banners: false,
            cache_copper_golem_statues: false,
            chunk_updates: 0,
            upload_budget_kib: 0,
            texture_budget_mib: 0,
            sky: true,
            fog: true,
            weather: true,
            animated_textures: true,
            dither_cutout_mips: false,
            present_mode: PresentMode::Auto,
            pacing: Pacing::Efficient,
            workers: 0,
            overlay: Overlay::Off,
            resolution_scale: 100,
            fxaa: false,
            taa: false,
            upscaling: Upscaling::Native,
            upscaling_sharpness: 85,
            screenshot_quality: 1,
            frame_generation: FrameGeneration::Off,
            hdr: HdrMode::Off,
            hdr_paper_white: 0,
            hdr_peak: 0,
            hdr_black_level: 0,
            hdr_ui_brightness: 100,
            hdr_lighting: true,
            hdr_key: "key.keyboard.unknown".into(),
            hdr_failed_start: None,
            battery: false,
            shadow_cache: false,
            api: GraphicsApi::Auto,
            translucent_batching: true,
            hdr_smooth_gradients: true,
            shader_pack: String::new(),
        }
    }
}
fn bool_text(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}
impl Graphics {
    pub fn load(dir: &Path) -> Self {
        let mut g: Self = fs::read_to_string(dir.join(FILE))
            .map(|t| Self::parse(&t))
            .unwrap_or_default();
        g.blue_map.servers = match fs::read_to_string(dir.join(crate::bluemap::SERVERS_FILE)) {
            Ok(text) => crate::bluemap::Settings::parse_servers(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                log::warn!("{}: {e}; no BlueMap is named", crate::bluemap::SERVERS_FILE);
                Vec::new()
            }
        };
        let marker = dir.join(HDR_MARKER);
        if let Ok(text) = fs::read_to_string(&marker)
            && fs::remove_file(&marker).is_ok()
            && g.hdr != HdrMode::Off
        {
            g.hdr_failed_start = Some(marker_driver(&text).to_owned());
            g.save(dir);
            log::warn!(
                "the last start ended before HDR output showed a frame; HDR output stays off with that graphics driver until the HDR mode is changed (graphics option hdrFailedStart, in {FILE})"
            );
        }
        g
    }
    /// Whether HDR output is held back on a window whose graphics driver is `driver`
    /// (`gfx::output::window_driver`): the last start on it ended before HDR output
    /// showed a frame ([`Graphics::hdr_failed_start`]).
    pub fn hdr_held(&self, driver: &str) -> bool {
        self.hdr_failed_start
            .as_deref()
            .is_some_and(|failed| failed.is_empty() || failed == driver)
    }
    /// The window's renderer runs on `driver`: a failed start that named no driver is this
    /// one's now, and another driver's (a driver update, another GPU) is dropped, so HDR output
    /// is tried again. True when it changed.
    pub fn settle_failed_start(&mut self, driver: &str) -> bool {
        match self.hdr_failed_start.as_deref() {
            Some("") => self.hdr_failed_start = Some(driver.to_owned()),
            Some(failed) if failed != driver => self.hdr_failed_start = None,
            _ => return false,
        }
        true
    }
    pub fn parse(text: &str) -> Self {
        let mut g = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            let flag = |d: bool| match value {
                "true" => true,
                "false" => false,
                _ => d,
            };
            let number = |d: u32| value.parse().unwrap_or(d);
            match key.trim() {
                "preset" => g.preset = Preset::parse(value),
                "lodDistance" => {
                    g.lod_distance = number(g.lod_distance).min(crate::lod::MAX_CHUNKS)
                }
                "farDetail" => g.far_detail = FarDetail::parse(value),
                "blueMapRadius" => {
                    if let Some(radius) = crate::bluemap::Settings::parse_radius(value) {
                        g.blue_map.radius = radius;
                    }
                }
                "blueMapAutoDetect" => g.blue_map.auto_detect = flag(g.blue_map.auto_detect),
                "caveCulling" => g.cave_culling = flag(g.cave_culling),
                "gpuCulling" => g.gpu_culling = GpuCullingMode::parse(value, g.gpu_culling),
                "entityCulling" => g.entity_culling = EntityCullingMode::parse(value),
                "entityCullingPrecheck" => {
                    g.entity_culling_precheck = number(g.entity_culling_precheck).min(512)
                }
                "particleCulling" => g.particle_culling = ParticleCullingMode::parse(value),
                "leafCulling" => g.leaf_culling = LeafCullingMode::parse(value),
                "opaqueIce" => g.opaque_ice = flag(g.opaque_ice),
                "blockEntityCache" => {
                    let enabled = flag(false);
                    g.cache_shulker_boxes = enabled;
                    g.cache_signs = enabled;
                    g.cache_decorated_pots = enabled;
                    g.cache_bells = enabled;
                    g.cache_chests = enabled;
                    g.cache_banners = enabled;
                    g.cache_copper_golem_statues = enabled;
                }
                "blockEntityCacheShulkerBoxes" => {
                    g.cache_shulker_boxes = flag(g.cache_shulker_boxes)
                }
                "blockEntityCacheSigns" => g.cache_signs = flag(g.cache_signs),
                "blockEntityCacheDecoratedPots" => {
                    g.cache_decorated_pots = flag(g.cache_decorated_pots)
                }
                "blockEntityCacheBells" => g.cache_bells = flag(g.cache_bells),
                "blockEntityCacheChests" => g.cache_chests = flag(g.cache_chests),
                "blockEntityCacheBanners" => g.cache_banners = flag(g.cache_banners),
                "blockEntityCacheCopperGolemStatues" => {
                    g.cache_copper_golem_statues = flag(g.cache_copper_golem_statues)
                }
                "chunkUpdates" => g.chunk_updates = number(g.chunk_updates).min(512),
                "uploadBudgetKiB" => g.upload_budget_kib = number(g.upload_budget_kib).min(65536),
                "textureBudgetMiB" => {
                    g.texture_budget_mib = number(g.texture_budget_mib).min(16384);
                }
                "sky" => g.sky = flag(g.sky),
                "fog" => g.fog = flag(g.fog),
                "weather" => g.weather = flag(g.weather),
                "animatedTextures" => g.animated_textures = flag(g.animated_textures),
                "ditherCutoutMips" => g.dither_cutout_mips = flag(g.dither_cutout_mips),
                "presentMode" => g.present_mode = PresentMode::parse(value),
                "framePacing" => {
                    g.pacing = if value == "precise" {
                        Pacing::Precise
                    } else {
                        Pacing::Efficient
                    }
                }
                "workers" => g.workers = number(g.workers).min(64),
                "resolutionScale" => g.resolution_scale = number(g.resolution_scale).clamp(50, 200),
                "fxaa" => g.fxaa = flag(g.fxaa),
                "taa" => g.taa = flag(g.taa),
                "upscaling" => g.upscaling = Upscaling::parse(value),
                "upscalingSharpness" => {
                    g.upscaling_sharpness = number(g.upscaling_sharpness).min(100)
                }
                "screenshotQuality" => {
                    g.screenshot_quality = Some(number(0))
                        .filter(|q| SCREENSHOT_QUALITIES.contains(q))
                        .unwrap_or(1)
                }
                "frameGeneration" => g.frame_generation = FrameGeneration::parse(value),
                "hdr" => g.hdr = HdrMode::parse(value, g.hdr),
                "hdrLighting" => g.hdr_lighting = flag(g.hdr_lighting),
                "hdrPaperWhite" => {
                    g.hdr_paper_white = match number(g.hdr_paper_white) {
                        0 => 0,
                        n => n.clamp(10, 480),
                    }
                }
                "hdrPeak" => {
                    g.hdr_peak = match number(g.hdr_peak) {
                        0 => 0,
                        n => n.clamp(200, 10_000),
                    }
                }
                "hdrBlackLevel" => g.hdr_black_level = number(g.hdr_black_level).min(100),
                "hdrUiBrightness" => {
                    g.hdr_ui_brightness = number(g.hdr_ui_brightness).clamp(25, 100)
                }
                "hdrKey" if value.starts_with("key.") => g.hdr_key = value.to_owned(),
                "hdrFailedStart" => g.hdr_failed_start = Some(value.to_owned()),
                "batteryMode" => g.battery = flag(g.battery),
                "shadowCache" => g.shadow_cache = flag(g.shadow_cache),
                "batchTranslucent" => g.translucent_batching = flag(g.translucent_batching),
                "hdrSmoothGradients" => g.hdr_smooth_gradients = flag(g.hdr_smooth_gradients),
                "graphicsApi" => g.api = GraphicsApi::from_name(value).unwrap_or_default(),
                "pickedShaderPack" => g.shader_pack = value.to_owned(),
                "overlay" => {
                    g.overlay = match value {
                        "fps" => Overlay::Fps,
                        "graph" => Overlay::Graph,
                        _ => Overlay::Off,
                    }
                }
                _ => {}
            }
        }
        g
    }
    pub fn serialize(&self) -> String {
        let mut text = format!(
            "preset:{}\nlodDistance:{}\nfarDetail:{}\ncaveCulling:{}\nentityCulling:{}\nentityCullingPrecheck:{}\nparticleCulling:{}\nleafCulling:{}\nopaqueIce:{}\nblockEntityCacheShulkerBoxes:{}\nblockEntityCacheSigns:{}\nblockEntityCacheDecoratedPots:{}\nblockEntityCacheBells:{}\nblockEntityCacheChests:{}\nblockEntityCacheBanners:{}\nblockEntityCacheCopperGolemStatues:{}\nchunkUpdates:{}\nuploadBudgetKiB:{}\ntextureBudgetMiB:{}\nsky:{}\nfog:{}\nweather:{}\nanimatedTextures:{}\nditherCutoutMips:{}\npresentMode:{}\nframePacing:{}\nworkers:{}\noverlay:{}\nresolutionScale:{}\nfxaa:{}\ntaa:{}\nupscaling:{}\nupscalingSharpness:{}\nscreenshotQuality:{}\nframeGeneration:{}\nhdr:{}\nhdrPaperWhite:{}\nhdrPeak:{}\nhdrBlackLevel:{}\nhdrUiBrightness:{}\nhdrLighting:{}\nhdrKey:{}\nbatteryMode:{}\nshadowCache:{}\ngraphicsApi:{}\npickedShaderPack:{}\nbatchTranslucent:{}\nhdrSmoothGradients:{}\ngpuCulling:{}\n",
            self.preset.name(),
            self.lod_distance,
            self.far_detail.name(),
            bool_text(self.cave_culling),
            self.entity_culling.name(),
            self.entity_culling_precheck,
            self.particle_culling.name(),
            self.leaf_culling.name(),
            bool_text(self.opaque_ice),
            bool_text(self.cache_shulker_boxes),
            bool_text(self.cache_signs),
            bool_text(self.cache_decorated_pots),
            bool_text(self.cache_bells),
            bool_text(self.cache_chests),
            bool_text(self.cache_banners),
            bool_text(self.cache_copper_golem_statues),
            self.chunk_updates,
            self.upload_budget_kib,
            self.texture_budget_mib,
            bool_text(self.sky),
            bool_text(self.fog),
            bool_text(self.weather),
            bool_text(self.animated_textures),
            bool_text(self.dither_cutout_mips),
            self.present_mode.name(),
            match self.pacing {
                Pacing::Efficient => "efficient",
                Pacing::Precise => "precise",
            },
            self.workers,
            match self.overlay {
                Overlay::Off => "off",
                Overlay::Fps => "fps",
                Overlay::Graph => "graph",
            },
            self.resolution_scale,
            bool_text(self.fxaa),
            bool_text(self.taa),
            self.upscaling.name(),
            self.upscaling_sharpness,
            self.screenshot_quality,
            self.frame_generation.name(),
            self.hdr.key(),
            self.hdr_paper_white,
            self.hdr_peak,
            self.hdr_black_level,
            self.hdr_ui_brightness,
            bool_text(self.hdr_lighting),
            self.hdr_key,
            bool_text(self.battery),
            bool_text(self.shadow_cache),
            self.api.name(),
            self.shader_pack,
            bool_text(self.translucent_batching),
            bool_text(self.hdr_smooth_gradients),
            self.gpu_culling.key(),
        );
        if let Some(driver) = &self.hdr_failed_start {
            text += &format!("hdrFailedStart:{driver}\n");
        }
        text += &self.blue_map.serialize();
        text
    }
    /// Binds Horizon's graphics key `name` (`HDR_KEY`); false for another name.
    pub fn set_key(&mut self, name: &str, value: String) -> bool {
        match name {
            HDR_KEY => self.hdr_key = value,
            _ => return false,
        }
        true
    }
    pub fn save(&self, dir: &Path) {
        let _ = fs::write(dir.join(FILE), self.serialize());
    }
    /// Paper white, in nits: the player's, or on Auto the desktop's SDR white as the
    /// system `reported` it for the renderer's display, else 203 (BT.2408's reference white).
    pub fn paper_white_nits(&self, reported: Option<Reported>) -> u32 {
        match self.hdr_paper_white {
            0 => reported.map_or(203, |r| r.paper_white),
            n => n,
        }
    }
    /// The peak the world's highlights reach, in nits: the player's, or on Auto
    /// what the system `reported`, else 1000.
    pub fn peak_nits(&self, reported: Option<Reported>) -> u32 {
        match self.hdr_peak {
            0 => reported.map_or(1000, |r| r.peak),
            n => n,
        }
    }
    /// The black level, in nits. Not the system's: files from before saved 0 for that
    /// (Auto), and a reported black would lift every display's shadows by default.
    pub fn black_level_nits(&self) -> f32 {
        self.hdr_black_level as f32 / 100.0
    }
    /// Worker threads for chunk generation and meshing.
    pub fn worker_threads(&self) -> usize {
        if self.workers > 0 {
            return self.workers as usize;
        }
        // Vanilla's count (Util.maxAllowedExecutorThreads: every core but
        // one). The pool never takes every core: a pool that fills them all
        // starves the render and server threads, and the frame rate collapses
        // while the workers saturate the CPU and its memory bandwidth. One
        // core left is enough with the workers behind those two threads
        // (horizon_threads: niced on Linux; on macOS and Windows the server
        // and render threads are raised above four cores, where vanilla
        // raises its server thread). Leaving two idled a core of a 4-thread
        // laptop while a world loaded: a teleport there took ~15% longer. On Apple
        // silicon only the performance cores count (horizon_threads::default_workers).
        let auto = horizon_threads::jobs::default_threads();
        if self.battery { auto.div_ceil(2) } else { auto }
    }
    /// The frame cap battery mode adds: 60 in a world, 30 in menus without a
    /// world or while another window has focus.
    pub fn battery_limit(&self, limit: i32, in_world: bool, focused: bool) -> i32 {
        if !self.battery {
            limit
        } else if in_world && focused {
            limit.min(60)
        } else {
            limit.min(30)
        }
    }
}

/// A preset's values for vanilla's options and Horizon's own.
pub fn apply_preset(preset: Preset, s: &mut crate::settings::Settings) {
    let (rd, sim, clouds, particles, shadows, blend, mipmaps, entity, leaves, weather_r) =
        match preset {
            Preset::Performance => (8, 6, "false", 2, false, 0, 0, 0.5, false, 3),
            Preset::Balanced => (12, 8, "fast", 1, true, 1, 2, 0.75, true, 5),
            Preset::Quality => (16, 12, "true", 0, true, 2, 4, 1.0, true, 10),
            Preset::Ultra => (24, 12, "true", 0, true, 3, 4, 1.5, true, 10),
            Preset::Custom => return,
        };
    s.render_distance = rd;
    s.simulation_distance = sim;
    s.clouds = clouds.into();
    s.particles = particles;
    s.entity_shadows = shadows;
    s.biome_blend = blend;
    s.mipmap_levels = mipmaps;
    s.entity_distance = entity;
    s.cutout_leaves = leaves;
    s.weather_radius = weather_r;
    s.graphics_preset = "custom".into();
    let g = &mut s.horizon;
    g.preset = preset;
    g.cave_culling = true;
    g.animated_textures = true;
    g.chunk_updates = 0;
    g.upload_budget_kib = 0;
    g.sky = true;
    g.fog = true;
    g.weather = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_modes_keep_boolean_files_and_round_trip_auto() {
        assert_eq!(Graphics::parse("").hdr, HdrMode::Off);
        for (value, expected) in [
            ("true", HdrMode::On),
            ("false", HdrMode::Off),
            ("on", HdrMode::On),
            ("off", HdrMode::Off),
            ("auto", HdrMode::Auto),
            ("unknown", HdrMode::Off),
        ] {
            let g = Graphics::parse(&format!("hdr:{value}\n"));
            assert_eq!(g.hdr, expected, "{value}");
            assert_eq!(Graphics::parse(&g.serialize()).hdr, expected);
            let saved = g.serialize();
            let value = saved.lines().find_map(|l| l.strip_prefix("hdr:")).unwrap();
            // The old boolean parser keeps its Off default for an unknown value.
            assert_eq!(value == "true", expected == HdrMode::On);
        }
        assert_eq!(Graphics::parse("hdr:true\nhdr:unknown\n").hdr, HdrMode::On);
    }

    #[test]
    fn the_api_choice_matches_the_system() {
        let choices = GraphicsApi::choices();
        if cfg!(windows) {
            assert_eq!(choices, GraphicsApi::ALL);
        } else if cfg!(target_os = "macos") {
            assert_eq!(choices, [GraphicsApi::Auto]);
        } else {
            assert_eq!(
                choices,
                [GraphicsApi::Auto, GraphicsApi::Vulkan, GraphicsApi::OpenGl]
            );
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A start that finds the marker (HDR output took the last one down) keeps the mode the
    /// player chose and saves the driver the marker names; HDR Off saves nothing.
    #[test]
    fn a_marker_keeps_the_mode_and_holds_the_driver_it_names() {
        let driver = "Vulkan 10de:2684 NVIDIA GeForce RTX 4090; NVIDIA; 581.57";
        for mode in HdrMode::ALL {
            let dir = scratch(&format!("hdr-marker-{mode:?}"));
            let saved = Graphics {
                hdr: mode,
                ..Graphics::default()
            };
            saved.save(&dir);
            fs::write(dir.join(HDR_MARKER), hdr_marker_text(driver)).unwrap();
            let g = Graphics::load(&dir);
            let failed = (mode != HdrMode::Off).then(|| driver.to_owned());
            assert_eq!((g.hdr, &g.hdr_failed_start), (mode, &failed), "{mode:?}");
            assert!(!dir.join(HDR_MARKER).exists(), "{mode:?}");
            // Saved for good: the next start, without a marker, reads the same.
            let again = Graphics::load(&dir);
            assert_eq!((again.hdr, again.hdr_failed_start), (mode, failed));
            let _ = fs::remove_dir_all(&dir);
        }
    }

    /// Today's marker names no driver: it holds whichever driver the window starts on first,
    /// and is that driver's from then on.
    #[test]
    fn a_marker_without_a_driver_holds_the_first_one_seen() {
        let dir = scratch("hdr-marker-legacy");
        Graphics {
            hdr: HdrMode::Auto,
            ..Graphics::default()
        }
        .save(&dir);
        fs::write(dir.join(HDR_MARKER), "HDR output was being switched on\n").unwrap();
        let mut g = Graphics::load(&dir);
        assert_eq!(
            (g.hdr, g.hdr_failed_start.as_deref()),
            (HdrMode::Auto, Some(""))
        );
        let (first, second) = (
            "Vulkan 10de:2684 GPU; NVIDIA; 1",
            "Dx12 1002:744c GPU; AMD; 2",
        );
        assert!(g.hdr_held(first) && g.hdr_held(second));
        assert!(g.settle_failed_start(first));
        assert!(g.hdr_held(first) && !g.hdr_held(second));
        assert!(!g.settle_failed_start(first));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The window starting on another driver (an update, another GPU) drops the hold, so HDR
    /// output is tried again; on the same driver it stays.
    #[test]
    fn another_driver_drops_the_hold() {
        let held = "Vulkan 10de:2684 GPU; NVIDIA; 581.57";
        let mut g = Graphics {
            hdr: HdrMode::On,
            hdr_failed_start: Some(held.into()),
            ..Graphics::default()
        };
        assert!(!g.settle_failed_start(held));
        assert!(g.hdr_held(held));
        assert!(g.settle_failed_start("Vulkan 10de:2684 GPU; NVIDIA; 582.02"));
        assert_eq!((g.hdr, g.hdr_failed_start), (HdrMode::On, None));
        assert!(!Graphics::default().hdr_held(held));
    }

    #[test]
    fn round_trips() {
        let g = Graphics {
            preset: Preset::Balanced,
            lod_distance: 256,
            far_detail: FarDetail::Reduced,
            blue_map: crate::bluemap::Settings {
                radius: 40,
                auto_detect: true,
                // In their own file (`bluemap::SERVERS_FILE`).
                servers: Vec::new(),
            },
            cave_culling: false,
            gpu_culling: GpuCullingMode::On,
            entity_culling: EntityCullingMode::Synchronous,
            entity_culling_precheck: 128,
            particle_culling: ParticleCullingMode::Asynchronous,
            leaf_culling: LeafCullingMode::SolidAggressive,
            opaque_ice: true,
            cache_shulker_boxes: true,
            cache_signs: true,
            cache_decorated_pots: true,
            cache_bells: true,
            cache_chests: true,
            cache_banners: true,
            cache_copper_golem_statues: true,
            chunk_updates: 12,
            upload_budget_kib: 4096,
            texture_budget_mib: 1536,
            sky: false,
            fog: false,
            weather: false,
            animated_textures: false,
            dither_cutout_mips: true,
            present_mode: PresentMode::Mailbox,
            pacing: Pacing::Precise,
            workers: 6,
            overlay: Overlay::Graph,
            resolution_scale: 75,
            fxaa: true,
            taa: true,
            upscaling: Upscaling::Balanced,
            upscaling_sharpness: 40,
            screenshot_quality: 4,
            frame_generation: FrameGeneration::Fsr31,
            hdr: HdrMode::On,
            hdr_paper_white: 240,
            hdr_peak: 1400,
            hdr_black_level: 12,
            hdr_ui_brightness: 60,
            hdr_lighting: false,
            hdr_key: "key.keyboard.h".into(),
            hdr_failed_start: Some(
                "Vulkan 10de:2684 NVIDIA GeForce RTX 4090; NVIDIA; 581.57".into(),
            ),
            battery: true,
            shadow_cache: true,
            api: GraphicsApi::Dx12,
            translucent_batching: false,
            hdr_smooth_gradients: false,
            shader_pack: "Golden Hour".into(),
        };
        assert_eq!(Graphics::parse(&g.serialize()), g);
        // A legacy marker's hold, which names no driver, round-trips too; none writes no line.
        for failed in [Some(String::new()), None] {
            let graphics = Graphics {
                hdr_failed_start: failed,
                ..Graphics::default()
            };
            assert_eq!(Graphics::parse(&graphics.serialize()), graphics);
        }
        assert!(!Graphics::default().serialize().contains("hdrFailedStart"));
        for mode in EntityCullingMode::ALL {
            let graphics = Graphics {
                entity_culling: mode,
                ..Graphics::default()
            };
            assert_eq!(Graphics::parse(&graphics.serialize()), graphics);
        }
        for mode in ParticleCullingMode::ALL {
            let graphics = Graphics {
                particle_culling: mode,
                ..Graphics::default()
            };
            assert_eq!(
                Graphics::parse(&graphics.serialize()).particle_culling,
                mode
            );
        }
        assert_eq!(
            Graphics::parse("entityCulling:true\n").entity_culling,
            EntityCullingMode::Synchronous
        );
        assert_eq!(
            Graphics::parse("entityCulling:false\n").entity_culling,
            EntityCullingMode::Off
        );
        // A map named here (a shared instance's, a pack's) is no map.
        assert!(
            Graphics::parse("blueMap:play.example.net https://map.example.net\n")
                .blue_map
                .servers
                .is_empty()
        );
        assert_eq!(Graphics::parse(""), Graphics::default());
        // Translucent Batching is on, in files from before it too; only false turns it off,
        // and only under its key: the old one held the false every save wrote while it was
        // off by default, not a choice.
        assert!(Graphics::default().translucent_batching);
        assert!(!Graphics::parse("batchTranslucent:false\n").translucent_batching);
        // Smooth Gradients is on; its 16-bit frame needs HDR on too (`output::resolve_frame`).
        assert!(Graphics::default().hdr_smooth_gradients);
        assert!(!Graphics::parse("hdr:true\nhdrSmoothGradients:false\n").hdr_smooth_gradients);
        assert!(Graphics::parse("translucentBatching:false\n").translucent_batching);
        // GPU Culling is opt-in: only the file or its switch turns it on.
        assert_eq!(Graphics::default().gpu_culling, GpuCullingMode::Off);
        assert_eq!(
            Graphics::parse("gpuCulling:true\n").gpu_culling,
            GpuCullingMode::On
        );
        // Auto is a third value of the same key. A file from before it reads as before, and an
        // older Horizon reading `auto` finds a value it doesn't know and keeps Off.
        assert_eq!(
            Graphics::parse("gpuCulling:auto\n").gpu_culling,
            GpuCullingMode::Auto
        );
        assert_eq!(
            Graphics::parse("gpuCulling:false\n").gpu_culling,
            GpuCullingMode::Off
        );
        assert_eq!(
            Graphics::parse("gpuCulling:maybe\n").gpu_culling,
            GpuCullingMode::Off
        );
        for mode in GpuCullingMode::ALL {
            let g = Graphics {
                gpu_culling: mode,
                ..Graphics::default()
            };
            assert_eq!(Graphics::parse(&g.serialize()).gpu_culling, mode);
        }
        // 0 is Auto; anything else is a peak, up to PQ's 10 000 nits. Older files' values
        // (up to 2000) read as they were saved.
        for (file, peak) in [
            ("hdrPeak:0\n", 0),
            ("hdrPeak:50\n", 200),
            ("hdrPeak:1234\n", 1234),
            ("hdrPeak:2000\n", 2000),
            ("hdrPeak:3000\n", 3000),
            ("hdrPeak:10000\n", 10_000),
            ("hdrPeak:25000\n", 10_000),
        ] {
            let g = Graphics::parse(file);
            assert_eq!(g.hdr_peak, peak, "{file}");
            assert_eq!(Graphics::parse(&g.serialize()).hdr_peak, peak, "{file}");
        }
        assert_eq!(Graphics::parse("hdrPeak:3000\n").peak_nits(None), 3000);
        // The black level is what's saved, in hundredths of a nit: 0 (Auto in older files)
        // lifts nothing, and the most is a nit.
        for (file, hundredths, nits) in [
            ("", 0, 0.0),
            ("hdrBlackLevel:0\n", 0, 0.0),
            ("hdrBlackLevel:5\n", 5, 0.05),
            ("hdrBlackLevel:100\n", 100, 1.0),
            ("hdrBlackLevel:250\n", 100, 1.0),
        ] {
            let g = Graphics::parse(file);
            assert_eq!(g.hdr_black_level, hundredths, "{file}");
            assert_eq!(g.black_level_nits(), nits, "{file}");
            assert_eq!(Graphics::parse(&g.serialize()), g, "{file}");
        }
        assert_eq!(Graphics::parse("hdrPaperWhite:0\n").hdr_paper_white, 0);
        assert_eq!(Graphics::parse("hdrPaperWhite:5\n").hdr_paper_white, 10);
        // Only the button's choices.
        assert_eq!(
            Graphics::parse("screenshotQuality:8\n").screenshot_quality,
            8
        );
        assert_eq!(
            Graphics::parse("screenshotQuality:3\n").screenshot_quality,
            1
        );
        assert_eq!(
            Graphics::parse("screenshotQuality:16\n").screenshot_quality,
            1
        );
        // The old menus' Simple / Advanced choice is ignored, and so is the
        // shader pack older versions saved.
        assert_eq!(Graphics::parse("level:advanced\n"), Graphics::default());
        assert_eq!(
            Graphics::parse("shaderPack:Pack.zip\n"),
            Graphics::default()
        );
        assert_eq!(
            Graphics::parse(
                "workers:abc\nsky:maybe\ncullLeaves:true\nentityCulling:maybe\nleafCulling:maybe\n"
            ),
            Graphics::default()
        );
    }

    #[test]
    fn peak_scale_steps_by_50_to_2000_then_by_500_to_10000() {
        let scale: Vec<u32> = (0..=PEAK_STEPS).map(peak_nits_at).collect();
        let fine: Vec<u32> = (200..=2000).step_by(50).collect();
        let coarse: Vec<u32> = (2500..=10_000).step_by(500).collect();
        assert_eq!(scale, [fine, coarse].concat());
        for (step, &nits) in scale.iter().enumerate() {
            assert_eq!(peak_step_of(nits as f32), step as f32);
        }
        // Off the scale, between its neighbours; beyond it, at its ends.
        assert_eq!(peak_step_of(1225.0), 20.5);
        assert_eq!(peak_step_of(3250.0), 38.5);
        assert_eq!(peak_step_of(100.0), 0.0);
        assert_eq!(peak_step_of(20_000.0), PEAK_STEPS as f32);
    }

    #[test]
    fn leaf_culling_round_trips_its_stable_names() {
        assert_eq!(Graphics::default().leaf_culling, LeafCullingMode::Off);
        for mode in LeafCullingMode::ALL {
            let g = Graphics {
                leaf_culling: mode,
                ..Graphics::default()
            };
            assert_eq!(Graphics::parse(&g.serialize()), g, "{mode:?}");
            assert_eq!(
                Graphics::parse(&format!("leafCulling:{}\n", mode.name())).leaf_culling,
                mode
            );
        }
        for text in [
            "leafCulling:other\n",
            "leafCulling:\n",
            "leafCulling:SOLID\n",
        ] {
            assert_eq!(
                Graphics::parse(text).leaf_culling,
                LeafCullingMode::Off,
                "{text:?}"
            );
        }
    }

    #[test]
    fn presets_keep_the_leaf_culling_choice() {
        for mode in LeafCullingMode::ALL {
            for preset in Preset::ALL {
                let mut s = crate::settings::Settings::default();
                s.horizon.leaf_culling = mode;
                apply_preset(preset, &mut s);
                assert_eq!(s.horizon.leaf_culling, mode, "{preset:?}");
                assert_eq!(Graphics::parse(&s.horizon.serialize()), s.horizon);
            }
        }
    }

    #[test]
    fn custom_packs_turn_leaf_culling_off() {
        let g = Graphics {
            leaf_culling: LeafCullingMode::Hollow,
            ..Graphics::default()
        };
        assert!(g.vanilla_models(&["vanilla".into()]));
        assert_eq!(
            g.leaf_culling_effective(&["vanilla".into()]),
            LeafCullingMode::Hollow
        );
        assert!(!g.vanilla_models(&["vanilla".into(), "My Pack".into()]));
        assert_eq!(
            g.leaf_culling_effective(&["vanilla".into(), "My Pack".into()]),
            LeafCullingMode::Off
        );
        assert_eq!(g.leaf_culling_effective(&[]), LeafCullingMode::Hollow);
    }

    #[test]
    fn server_pack_apply_disables_leaf_culling_and_removal_restores_it() {
        let g = Graphics {
            leaf_culling: LeafCullingMode::Solid,
            ..Graphics::default()
        };
        let selected = ["vanilla"];
        let applied_server_pack = ["server/0123456789abcdef/model-pack.zip"];

        assert_eq!(
            g.leaf_culling_effective_with_server_packs(selected, applied_server_pack),
            LeafCullingMode::Off,
            "an applied server pack can replace block models"
        );
        assert!(!g.vanilla_models_with_server_packs(selected, applied_server_pack));

        assert_eq!(
            g.leaf_culling_effective_with_server_packs(selected, []),
            LeafCullingMode::Solid,
            "removing the server pack restores the selected local mode"
        );
        assert!(g.vanilla_models_with_server_packs(selected, []));
    }

    #[test]
    fn integrated_gpus_other_than_apple_cap_the_lod_distance() {
        let adapter = |name: &str, device_type| wgpu::AdapterInfo {
            name: name.into(),
            vendor: 0,
            device: 0,
            device_type,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Vulkan,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        };
        let hd620 = adapter(
            "Intel(R) HD Graphics 620 (KBL GT2)",
            wgpu::DeviceType::IntegratedGpu,
        );
        let m4 = adapter("Apple M4 Pro", wgpu::DeviceType::IntegratedGpu);
        let discrete = adapter("NVIDIA GeForce RTX 4070", wgpu::DeviceType::DiscreteGpu);
        for (distance, capped) in [(0, 0), (64, 64), (128, 128), (256, 128), (1024, 128)] {
            let g = Graphics {
                lod_distance: distance,
                ..Graphics::default()
            };
            assert_eq!(g.lod_distance_on(&hd620), capped);
            assert_eq!(g.lod_distance_on(&m4), distance);
            assert_eq!(g.lod_distance_on(&discrete), distance);
        }
        assert!(
            crate::graphics_ui::rows()
                .into_iter()
                .flat_map(|(_, groups)| groups.into_iter().flat_map(|(_, rows)| rows))
                .any(|r| r
                    .text
                    .contains(&format!("{INTEGRATED_LOD_DISTANCE} chunks"))),
            "the LOD Distance description names the cap"
        );
    }

    #[test]
    fn lod_distance_is_opt_in_and_bounded() {
        assert_eq!(Graphics::default().lod_distance, 0);
        for distance in [0, 64, 128, 256, 512, 1024, 65_536, crate::lod::MAX_CHUNKS] {
            let g = Graphics::parse(&format!("lodDistance:{distance}\n"));
            assert_eq!(g.lod_distance, distance);
            assert_eq!(Graphics::parse(&g.serialize()), g);
        }
        assert_eq!(
            Graphics::parse("lodDistance:99999999\n").lod_distance,
            crate::lod::MAX_CHUNKS
        );
        assert_eq!(Graphics::parse("lodDistance:-1\n").lod_distance, 0);
        assert_eq!(Graphics::parse("lodDistance:invalid\n").lod_distance, 0);
    }

    #[test]
    fn far_detail_is_full_unless_reduced_is_picked() {
        assert_eq!(Graphics::default().far_detail, FarDetail::Full);
        assert_eq!(
            Graphics::parse("farDetail:reduced\n").far_detail,
            FarDetail::Reduced
        );
        for text in ["farDetail:full\n", "farDetail:other\n", ""] {
            assert_eq!(Graphics::parse(text).far_detail, FarDetail::Full);
        }
        // No preset changes it.
        for preset in Preset::ALL {
            let mut s = crate::settings::Settings::default();
            apply_preset(preset, &mut s);
            assert_eq!(s.horizon.far_detail, FarDetail::Full, "{preset:?}");
        }
    }

    #[test]
    fn every_preset_keeps_the_animated_textures_on() {
        // The Performance preset used to turn them off, which froze the fire,
        // the water, the lava and the portals (#2064).
        assert!(Graphics::default().animated_textures);
        for preset in Preset::ALL {
            let mut s = crate::settings::Settings::default();
            apply_preset(preset, &mut s);
            assert!(
                s.horizon.animated_textures,
                "{preset:?} turned the animated textures off"
            );
        }
    }

    #[test]
    fn automatic_workers_leave_a_core_as_vanilla_does() {
        let g = Graphics::default();
        assert_eq!(g.workers, 0);
        let cores = horizon_threads::worker_count();
        assert!((1..=cores.saturating_sub(1).max(1)).contains(&g.worker_threads()));
        if !cfg!(target_vendor = "apple") {
            assert_eq!(g.worker_threads(), cores.saturating_sub(1).max(1));
        }
        // A manual count is the player's own choice, kept as asked.
        let manual = Graphics { workers: 3, ..g };
        assert_eq!(manual.worker_threads(), 3);
    }

    #[test]
    fn battery_mode_only_lowers_the_frame_cap() {
        let mut g = Graphics::default();
        assert_eq!(g.battery_limit(260, true, true), 260);
        g.battery = true;
        assert_eq!(g.battery_limit(260, true, true), 60);
        assert_eq!(g.battery_limit(30, true, true), 30);
        assert_eq!(g.battery_limit(260, false, true), 30);
        assert_eq!(g.battery_limit(260, true, false), 30);
        assert_eq!(g.battery_limit(10, true, false), 10);
    }
}
