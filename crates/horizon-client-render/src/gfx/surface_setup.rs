//! Setting the window surface up on an adapter during the device search (`open_backend`):
//! an adapter that can't present to the window gives way to another of its backend
//! (#13493).
use super::{
    Renderer, check_layers, cull, describe_adapter, is_software, oit, startup_step, surface_format,
    timing,
};
// Ray tracing on request, which the device needs from its creation (gfx.rs may not grow).
#[path = "ray_query.rs"]
mod ray_query;
use std::sync::Arc;
use winit::window::Window;

/// Sets the window surface up on `device`: the swapchain format (`surface_format`), the
/// copies screenshots and frame generation make, and Mailbox when the surface offers it
/// (`keep_mode`, a replaced renderer's mode, first). `caps` are the surface's capabilities
/// on `adapter`. Errs, instead of wgpu's panic, when the surface can't be configured on
/// this device: a hybrid laptop's discrete GPU under Wayland accepts the surface in
/// `request_adapter` and fails only here (#13493).
#[allow(clippy::too_many_arguments)]
pub(super) fn set_up_surface(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    device_lost: &std::sync::atomic::AtomicBool,
    caps: wgpu::SurfaceCapabilities,
    size: winit::dpi::PhysicalSize<u32>,
    keep_mode: Option<wgpu::PresentMode>,
    log_step: bool,
) -> std::result::Result<(wgpu::SurfaceConfiguration, wgpu::SurfaceCapabilities), String> {
    let max_side = device.limits().max_texture_dimension_2d;
    let mut config = surface
        .get_default_config(
            adapter,
            size.width.clamp(1, max_side),
            size.height.clamp(1, max_side),
        )
        .ok_or("surface has no supported configuration")?;
    config.format = surface_format(&caps.formats, config.format);
    config.view_formats = vec![config.format.remove_srgb_suffix()];
    // Screenshots copy the presented frame back from the swapchain, frame
    // generation's present thread copies its frames in.
    if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
        config.usage |= wgpu::TextureUsages::COPY_SRC;
    }
    if caps.usages.contains(wgpu::TextureUsages::COPY_DST) {
        config.usage |= wgpu::TextureUsages::COPY_DST;
    }
    config.present_mode = keep_mode
        .filter(|mode| caps.present_modes.contains(mode))
        .unwrap_or_else(|| {
            if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            }
        });
    if log_step {
        startup_step(format_args!(
            "device ready; configuring the surface ({}x{} {:?}, {:?})",
            config.width, config.height, config.format, config.present_mode
        ));
    }
    Renderer::configure(device, surface, &config, None, device_lost)?;
    Ok((config, caps))
}
/// What `pick_adapter` knows of one of a backend's adapters.
#[derive(Clone, Copy, Debug)]
pub(super) struct AdapterChoice {
    pub(super) kind: wgpu::DeviceType,
    /// The window surface offers it formats (`Surface::get_capabilities`).
    pub(super) presents: bool,
    /// Already tried, and it could not present.
    pub(super) tried: bool,
}
/// The adapter to try after one that could not present to the window: one not tried yet
/// that the surface offers formats on, a software one only when `allow_cpu`, ranked as
/// `request_adapter` ranks for `HighPerformance` (discrete, integrated, other, virtual,
/// CPU), the first listed among equals. A hybrid laptop's integrated GPU drives the
/// Wayland compositor and presents when its discrete GPU can't (#13493).
pub(super) fn pick_adapter(adapters: &[AdapterChoice], allow_cpu: bool) -> Option<usize> {
    let rank = |kind| match kind {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::Other => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.presents && !a.tried && (allow_cpu || !is_software(a.kind)))
        .min_by_key(|(_, a)| rank(a.kind))
        .map(|(i, _)| i)
}
/// Why an adapter gave no device: it can't present to the window, so another adapter of
/// the backend may, or another failure.
pub(super) enum AdapterFailure {
    CannotPresent(String),
    Failed(String),
}
/// A device opened on one adapter, with the window surface set up on it.
pub(super) struct Adapted {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) device_lost: Arc<std::sync::atomic::AtomicBool>,
    pub(super) configured: Option<(wgpu::SurfaceConfiguration, wgpu::SurfaceCapabilities)>,
}
/// Opens a device on `adapter` and configures the window's `surface` on it (none
/// offscreen). An adapter the surface offers no format on fails before its device opens.
pub(super) async fn open_adapter(
    name: &str,
    window: &Arc<Window>,
    surface: Option<&wgpu::Surface<'static>>,
    adapter: &wgpu::Adapter,
    layers: u32,
) -> std::result::Result<Adapted, AdapterFailure> {
    let caps = surface.map(|surface| surface.get_capabilities(adapter));
    if caps.as_ref().is_some_and(|caps| caps.formats.is_empty()) {
        return Err(AdapterFailure::CannotPresent(
            "the window surface offers it no format".into(),
        ));
    }
    describe_adapter(adapter);
    let info = adapter.get_info();
    let limits = adapter.limits();
    if let Err(e) = check_layers(adapter, layers) {
        return Err(AdapterFailure::Failed(format!("{name}: {e}")));
    }
    // Mappable primary buffers: section pages that no longer fit in
    // video memory go to system memory. Shader packs' 16-bit normalized buffers (render
    // targets with the adapter's own format features) and R11F_G11F_B10F: a float
    // stand-in neither clamps nor drops NaN. Subgroups: shader packs' compute programs
    // (`MC_GL_KHR_shader_subgroup_*`); storage buffers in vertex shaders, clearing and
    // atomics on images: their buffers and images. Filtering 32-bit float textures: shader
    // packs' RGBA32F buffers, which GL filters and mipmaps. Count draws: GPU culling's (`gpu_culling`).
    let features = timing::GpuTimer::features(adapter)
        | cull::Cull::features(adapter)
        | oit::features(adapter)
        | ray_query::features(adapter)
        | (adapter.features()
            & (wgpu::Features::INDIRECT_FIRST_INSTANCE
                | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT
                | wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
                | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | wgpu::Features::RG11B10UFLOAT_RENDERABLE
                | wgpu::Features::SUBGROUP
                | wgpu::Features::VERTEX_WRITABLE_STORAGE
                | wgpu::Features::CLEAR_TEXTURE
                | wgpu::Features::TEXTURE_ATOMIC
                | wgpu::Features::FLOAT32_FILTERABLE));
    let driver = format!("{} {}", info.driver, info.driver_info);
    startup_step(format_args!(
        "requesting the device (driver {}) with features [{features}] and the adapter's \
         limits (buffers up to {} bytes, storage bindings up to {} bytes, {} texture layers)",
        match driver.trim() {
            "" => "unnamed",
            driver => driver,
        },
        limits.max_buffer_size,
        limits.max_storage_buffer_binding_size,
        limits.max_texture_array_layers,
    ));
    let desc = wgpu::DeviceDescriptor {
        label: Some("Horizon device"),
        required_features: features,
        required_limits: limits,
        // Video memory in blocks of 8-64 MiB instead of 128-256 MiB (Vulkan and DirectX 12):
        // with the larger blocks a few small buffers kept whole blocks the arena's pages had
        // left, and over half the video memory reserved was unused (1,344 MiB for 500).
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        experimental_features: ray_query::experimental(features),
        ..Default::default()
    };
    #[cfg(not(target_os = "macos"))]
    let opened = match info.backend {
        wgpu::Backend::Vulkan => horizon_hdr::open_vulkan_device(adapter, &desc),
        _ => None,
    };
    #[cfg(target_os = "macos")]
    let opened = None;
    let opened = match opened {
        Some(opened) => opened,
        None => adapter.request_device(&desc).await,
    };
    let (device, queue) = match opened {
        Ok(opened) => opened,
        Err(e) => return Err(AdapterFailure::Failed(format!("{name} device: {e}"))),
    };
    let device_lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lost = device_lost.clone();
    device.set_device_lost_callback(move |reason, message| {
        // A device destroyed on purpose was not lost.
        if reason == wgpu::DeviceLostReason::Destroyed {
            return;
        }
        log::warn!("Horizon renderer: the graphics device was lost ({reason:?}: {message})");
        lost.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let configured = match (surface, caps) {
        (Some(surface), Some(caps)) => Some(
            set_up_surface(
                surface,
                adapter,
                &device,
                &device_lost,
                caps,
                window.inner_size(),
                None,
                true,
            )
            .map_err(AdapterFailure::CannotPresent)?,
        ),
        _ => None,
    };
    Ok(Adapted {
        device,
        queue,
        device_lost,
        configured,
    })
}
#[cfg(test)]
mod tests {
    use super::{AdapterChoice, pick_adapter};
    /// #13493: a hybrid laptop under Wayland. The NVIDIA GPU can't present to the window,
    /// so the integrated GPU that drives the compositor is next, never the one that failed.
    #[test]
    fn an_adapter_that_cannot_present_gives_way_to_one_that_can() {
        use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};
        let a = |kind, presents, tried| AdapterChoice {
            kind,
            presents,
            tried,
        };
        // The NVIDIA GPU failed to configure, the Intel one presents: the Intel one.
        let hybrid = [a(DiscreteGpu, true, true), a(IntegratedGpu, true, false)];
        assert_eq!(pick_adapter(&hybrid, false), Some(1));
        // Among adapters that present, HighPerformance's order: discrete first, then
        // integrated, other, virtual; the first listed among equals.
        let many = [
            a(VirtualGpu, true, false),
            a(Other, true, false),
            a(IntegratedGpu, true, false),
            a(DiscreteGpu, false, false),
            a(DiscreteGpu, true, false),
            a(DiscreteGpu, true, false),
        ];
        assert_eq!(pick_adapter(&many, false), Some(4));
        assert_eq!(pick_adapter(&many[..4], false), Some(2));
        assert_eq!(pick_adapter(&many[..2], false), Some(1));
        // Nothing left that presents: the next backend (OpenGL) is tried.
        let none = [a(DiscreteGpu, true, true), a(IntegratedGpu, false, false)];
        assert_eq!(pick_adapter(&none, false), None);
        assert_eq!(pick_adapter(&[], false), None);
        // A software adapter only on the search's last, software-allowed try.
        let llvmpipe = [a(DiscreteGpu, true, true), a(Cpu, true, false)];
        assert_eq!(pick_adapter(&llvmpipe, false), None);
        assert_eq!(pick_adapter(&llvmpipe, true), Some(1));
    }
}
