//! The world pass after culling, and the renderer's GPU pass timing switches and readings.
use super::{Renderer, native_pack, pack_post, pack_timing};

pub(super) fn world_pass_after_culling<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    keep_depth: bool,
    mut timing: Option<&mut pack_timing::PassTiming>,
    name: &str,
    pixels: u64,
) -> pack_timing::CountedPass<'e> {
    let pass = after_culling_pass(
        encoder,
        view,
        depth,
        keep_depth,
        timing.as_deref_mut().and_then(|t| t.writes(name)),
    );
    pack_timing::counted(timing, pass, pixels, "before translucent")
}
/// The pixels of a depth attachment, for `fragments_per_pixel`.
pub(super) fn pixel_count(depth: &wgpu::TextureView) -> u64 {
    let texture = depth.texture();
    u64::from(texture.width()) * u64::from(texture.height())
}
pub(super) fn after_culling_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    keep_depth: bool,
    timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Horizon world, after culling"),
        timestamp_writes,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                // A shader pack reads it as depthtex0.
                store: if keep_depth {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

impl Renderer {
    /// Times the shader pack's passes on the GPU while `on` (the F3 overlay is open or a capture records them).
    pub fn set_pass_timing(&mut self, on: bool) {
        self.pass_timing = on;
    }
    /// Times the parts of Horizon's own passes while `on`.
    pub fn set_pass_detail(&mut self, on: bool) {
        self.pass_detail = on;
    }
    /// Times a frame at most once per `gap` while passes are timed (the F3 overlay needs far fewer
    /// samples than a measurement).
    pub fn set_pass_timing_gap(&mut self, gap: std::time::Duration) {
        self.pass_timing_gap = gap;
    }
    /// Keeps each timed frame's pass times for `take_timed_frames` while `on` (a capture records them).
    pub fn set_keep_timed_frames(&mut self, on: bool) {
        self.keep_timed_frames = on;
    }
    /// The frames both timers timed since the last call, without waiting on the GPU.
    pub fn take_timed_frames(&mut self) -> pack_timing::TimedFrames {
        let own = self
            .own_timing
            .as_mut()
            .map(pack_timing::PassTiming::take_frames)
            .unwrap_or_default();
        let pack = match Self::pass_timer(&mut self.pack_post, &mut self.native) {
            Some(Some(t)) => t.take_frames(),
            _ => Vec::new(),
        };
        pack_timing::TimedFrames { own, pack }
    }
    /// The running shader pack's timer, of either kind.
    pub(super) fn pass_timer<'a>(
        pack: &'a mut Option<(i32, Option<pack_post::PackPost>)>,
        native: &'a mut Option<native_pack::NativePack>,
    ) -> Option<&'a mut Option<pack_timing::PassTiming>> {
        match (pack, native) {
            (Some((_, Some(post))), _) => Some(&mut post.timing),
            (_, Some(native)) => Some(&mut native.timing),
            _ => None,
        }
    }
    /// Each pass of the running shader pack with its median GPU time in milliseconds, while
    /// they are timed (`set_pass_timing`).
    pub fn shader_pass_times(&self) -> Vec<(String, f32)> {
        let timer = match (&self.pack_post, &self.native) {
            (Some((_, Some(post))), _) => post.timing.as_ref(),
            (_, Some(native)) => native.timing.as_ref(),
            _ => None,
        };
        timer
            .map(pack_timing::PassTiming::medians)
            .unwrap_or_default()
    }
    /// Each of Horizon's own passes with its median GPU time in milliseconds, while they are
    /// timed (`set_pass_timing`).
    pub fn own_pass_times(&self) -> Vec<(String, f32)> {
        self.own_timing
            .as_ref()
            .map(pack_timing::PassTiming::medians)
            .unwrap_or_default()
    }
    /// Each of Horizon's own passes with its parts' median GPU times in milliseconds.
    pub fn own_pass_parts(&self) -> Vec<(String, Vec<(String, f32)>)> {
        self.own_timing
            .as_ref()
            .map(pack_timing::PassTiming::part_medians)
            .unwrap_or_default()
    }
    /// Each counted pass's counters (medians), while detail is on and the device has them.
    pub fn own_pass_counters(&self) -> Vec<(String, pack_timing::PassCounters)> {
        self.own_timing
            .as_ref()
            .map(pack_timing::PassTiming::counter_medians)
            .unwrap_or_default()
    }
    /// Both timers' readings and this adapter, for a capture; `None` when neither has timed a pass.
    pub fn gpu_readout(&self) -> Option<pack_timing::GpuReadout> {
        let own = self
            .own_timing
            .as_ref()
            .map(pack_timing::PassTiming::readings)
            .filter(|r| !r.is_empty());
        let pack = match (&self.pack_post, &self.native) {
            (Some((_, Some(post))), _) => post.timing.as_ref(),
            (_, Some(native)) => native.timing.as_ref(),
            _ => None,
        }
        .map(pack_timing::PassTiming::readings)
        .filter(|r| !r.is_empty());
        (own.is_some() || pack.is_some()).then(|| pack_timing::GpuReadout {
            adapter: self.info.clone(),
            own,
            pack,
        })
    }
    /// Whether dispatch parts and counters are being asked for (`set_pass_detail`).
    pub fn pass_detail(&self) -> bool {
        self.pass_detail
    }
    /// What detail timing can show on this device; `None` without pass timing.
    pub fn pass_detail_support(&self) -> Option<pack_timing::DetailSupport> {
        self.own_timing
            .as_ref()
            .map(pack_timing::PassTiming::detail_support)
    }
}
