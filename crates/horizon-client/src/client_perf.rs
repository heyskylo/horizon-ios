//! The client's .hzperf captures (docs/perf-platform.md §4.1): the window's frames feed the always-on health history, and a capture writes it with the samples of the render thread.
use crate::gfx::pack_timing::GpuReadout;
use crate::server_perf::traffic::Wire;
use crate::server_perf::{
    self, CaptureEnd, CaptureOptions, CaptureStatus, MAX_WINDOW_S, Role, SamplerOptions, Saved,
    ServerPerf, TickDetail, WorldCounts,
};
use horizon_perf_format::FieldSender;
use horizon_perf_format::v1::{
    BenchResult, Machine, Side, Start, StopReason, Trigger, WarningCode,
};
use request::{GpuSwitch, StartOptions, Threads};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub mod request;

#[cfg(test)]
mod tests;

/// A frame this slow enters the slow ring when `HORIZON_SPIKES` names no other threshold.
pub const SLOW_FRAME_MS: f32 = 50.0;

/// The variables that start a capture with the GPU passes timed: pass timing, and pass timing with its parts.
pub const ENV_PASS_TIMING: &str = "HORIZON_SHADER_PACK_TIMING";
pub const ENV_PASS_DETAIL: &str = "HORIZON_GPU_PASS_DETAIL";

/// The GPU switch a run's environment asks for; the detail variable wins over the timing one.
pub fn env_gpu(timing: Option<&OsStr>, detail: Option<&OsStr>) -> GpuSwitch {
    if detail.is_some() {
        GpuSwitch::Detail
    } else if timing.is_some() {
        GpuSwitch::Passes
    } else {
        GpuSwitch::Off
    }
}

/// Where a client's captures go: `HORIZON_PROFILE` when set, else `reports/` in the game home.
pub fn reports_dir(profile: Option<&OsStr>, home: &Path) -> PathBuf {
    match profile.filter(|p| !p.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join("reports"),
    }
}

/// What a side on another thread needs to write into the running capture's file.
#[derive(Clone)]
pub struct Join {
    pub sender: FieldSender,
    pub path: PathBuf,
    /// The instant every `t` of the capture counts from.
    pub origin: Instant,
    /// When the capture's window ends, and why it stops then.
    pub ends: Option<(Instant, StopReason)>,
    pub options: StartOptions,
    pub trigger: Trigger,
}

/// The client's health history, always on, and the capture running from it, if any.
pub struct ClientPerf {
    perf: ServerPerf,
    epoch: Instant,
    cpu_ms: Vec<f32>,
    gpu_ms: Vec<f32>,
    /// When the running capture's window ends, and why it stops then.
    window: Option<(f64, StopReason)>,
    /// The running capture's switches, which a side that joins it follows.
    running: Option<(StartOptions, Trigger)>,
}

impl ClientPerf {
    pub fn new(slow_ms: f32) -> Self {
        Self {
            perf: ServerPerf::with_role(Role::CLIENT, 0.0, slow_ms),
            epoch: Instant::now(),
            cpu_ms: Vec::new(),
            gpu_ms: Vec::new(),
            window: None,
            running: None,
        }
    }

    /// Seconds since the game began, the clock every other time here is on.
    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    pub fn capturing(&self) -> bool {
        self.perf.capturing()
    }

    pub fn health(&self) -> &horizon_perf_format::HealthRecorder {
        self.perf.health()
    }

    /// One rendered frame that took `wall_ms`, of which `cpu_ms` was the CPU's own work, ending at `t`.
    pub fn frame(
        &mut self,
        t: f64,
        wall_ms: f32,
        cpu_ms: f32,
        gpu_ms: Option<f32>,
        detail: Option<TickDetail>,
        world: impl FnOnce() -> WorldCounts,
    ) {
        self.frame_with_gpu(t, wall_ms, cpu_ms, gpu_ms, detail, world, || None);
    }

    /// [`ClientPerf::frame`], where `gpu` is asked for the pass timings only when this frame ends the capture's window.
    #[allow(clippy::too_many_arguments)]
    pub fn frame_with_gpu(
        &mut self,
        t: f64,
        wall_ms: f32,
        cpu_ms: f32,
        gpu_ms: Option<f32>,
        detail: Option<TickDetail>,
        world: impl FnOnce() -> WorldCounts,
        gpu: impl FnOnce() -> Option<GpuReadout>,
    ) {
        self.cpu_ms.push(cpu_ms);
        self.gpu_ms.extend(gpu_ms);
        if self.perf.gauges_due(t) {
            self.perf
                .set_frame_medians(median(&mut self.cpu_ms), median(&mut self.gpu_ms));
        }
        self.perf.tick(t, wall_ms, detail, world, Wire::new);
        if let Some((end, reason)) = self.window
            && t >= end
        {
            self.stop_with_gpu(t, reason, gpu());
        }
    }

    /// Opens a capture in `dir`; call it on the render thread, which it samples.
    pub fn start(
        &mut self,
        t: f64,
        dir: &Path,
        trigger: Trigger,
        options: &StartOptions,
        start: Start,
        block_names: Vec<String>,
    ) -> std::io::Result<PathBuf> {
        let capture = CaptureOptions {
            window_s: options.seconds.map(f64::from),
            trigger,
            start,
            samples: options.samples,
            sampler: SamplerOptions {
                hz: options.hz,
                over_ms: options.over_ms,
                wall: options.wall,
                all_threads: options.threads == Threads::All,
            },
            block_names,
            levels: Vec::new(),
            wire: Wire::new(),
            gpu: options.gpu.mode(),
            timeline: options.timeline.then(Default::default),
            file: options.file.clone(),
        };
        let path = self.perf.start_capture(t, dir, capture)?;
        self.window = Some(match options.seconds {
            Some(s) => (t + f64::from(s), StopReason::Normal),
            None => (t + MAX_WINDOW_S, StopReason::WindowCap),
        });
        self.running = Some((options.clone(), trigger));
        Ok(path)
    }

    /// What the server thread needs to write its side into the running capture; None when none runs.
    pub fn join(&self) -> Option<Join> {
        let (sender, path, origin) = self.perf.joinable()?;
        let (options, trigger) = self.running.clone()?;
        let at = |t: f64| self.epoch + Duration::from_secs_f64(t.max(0.0));
        Some(Join {
            sender,
            path,
            origin: at(origin),
            ends: self.window.map(|(end, reason)| (at(end), reason)),
            options,
            trigger,
        })
    }

    /// Ends the running capture; its file is finished off this thread and comes back from [`ClientPerf::take_saved`].
    pub fn stop(&mut self, t: f64, reason: StopReason) -> bool {
        self.stop_with(t, reason, None, Vec::new())
    }

    /// [`ClientPerf::stop`], recording `gpu`'s pass timings if the capture asked for them.
    pub fn stop_with_gpu(&mut self, t: f64, reason: StopReason, gpu: Option<GpuReadout>) -> bool {
        self.stop_with(t, reason, gpu, Vec::new())
    }

    /// [`ClientPerf::stop_with_gpu`] with benchmark reports embedded in the capture.
    pub fn stop_with(
        &mut self,
        t: f64,
        reason: StopReason,
        gpu: Option<GpuReadout>,
        bench: Vec<BenchResult>,
    ) -> bool {
        self.window = None;
        self.running = None;
        let end = CaptureEnd {
            gpu,
            bench,
            ..CaptureEnd::default()
        };
        self.perf.begin_stop(t, reason, end)
    }

    /// Whether a stopped capture's file is still being finished or waits to be taken.
    pub fn saving(&self) -> bool {
        self.perf.finishing()
    }

    /// The captures whose files are done; with `wait`, every one begun.
    pub fn take_saved(&mut self, wait: bool) -> Vec<std::io::Result<Saved>> {
        self.perf.take_saved(wait)
    }

    pub fn mark(&mut self, t: f64, name: &str) -> bool {
        self.perf.mark(t, name)
    }

    /// Records a memory snapshot in the running capture; false when none runs.
    pub fn memory_snapshot(&mut self, t: f64, label: &str, rows: &[(String, f64)]) -> bool {
        self.perf.memory_snapshot(t, label, rows)
    }

    /// Records `code` in the running capture; false when none runs.
    pub fn warn(&mut self, t: f64, code: WarningCode) -> bool {
        self.perf.warn(t, code)
    }

    pub fn status(&self, t: f64) -> Option<CaptureStatus> {
        self.perf.status(t)
    }

    /// The seconds left in the running capture's window.
    pub fn remaining_s(&self, t: f64) -> Option<f64> {
        self.window.map(|(end, _)| (end - t).max(0.0))
    }
}

/// A benchmark report as the capture embeds it, minus its local-only label and paths; anything but a `horizon-benchmark-report` is refused.
pub fn bench_result(report_json: String) -> Result<BenchResult, String> {
    let mut report = horizon_bench::Report::parse(&report_json)?;
    report.label = None;
    report.local.clear();
    Ok(BenchResult {
        side: Side::Client as i32,
        format: horizon_bench::FORMAT.to_owned(),
        report_json: report.to_json(),
        corpus_id: String::new(),
    })
}

/// The median of the values, which are cleared; None when there were none.
fn median(values: &mut Vec<f32>) -> Option<f32> {
    values.sort_by(f32::total_cmp);
    let rank = values.len().div_ceil(2);
    let m = rank.checked_sub(1).map(|i| values[i]);
    values.clear();
    m
}

/// `Start` for a client capture: its build and machine; the capture fills the rest.
pub fn start_message(same_process: bool, gpu: Option<&str>) -> Start {
    let mut machine: Machine = server_perf::machine(Some(
        server_perf::hardware().unwrap_or_else(server_perf::hardware_now),
    ));
    if let Some(gpu) = gpu {
        machine.gpu = gpu.to_owned();
    }
    Start {
        build: Some(server_perf::build()),
        machine: Some(machine),
        same_process,
        ..Start::default()
    }
}
