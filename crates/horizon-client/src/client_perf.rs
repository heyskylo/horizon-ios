//! The client's .hzperf captures (docs/perf-platform.md §4.1): the window's frames feed the always-on health history, and a capture writes it with the samples of the render thread.
use crate::server_perf::{
    self, CaptureOptions, CaptureStatus, MAX_WINDOW_S, Role, SamplerOptions, Saved, ServerPerf,
    SlowDetail, WorldCounts,
};
use horizon_perf_format::v1::{Machine, Start, StopReason, Trigger};
use request::{StartOptions, Threads};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub mod request;

#[cfg(test)]
mod tests;

/// A frame this slow enters the slow ring when `HORIZON_SPIKES` names no other threshold.
pub const SLOW_FRAME_MS: f32 = 50.0;

/// Where a client's captures go: `HORIZON_PROFILE` when set, else `reports/` in the game home.
pub fn reports_dir(profile: Option<&OsStr>, home: &Path) -> PathBuf {
    match profile.filter(|p| !p.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join("reports"),
    }
}

/// The client's health history, always on, and the capture running from it, if any.
pub struct ClientPerf {
    perf: ServerPerf,
    epoch: Instant,
    cpu_ms: Vec<f32>,
    gpu_ms: Vec<f32>,
    /// When the running capture's window ends, and why it stops then.
    window: Option<(f64, StopReason)>,
}

impl ClientPerf {
    pub fn new(slow_ms: f32) -> Self {
        Self {
            perf: ServerPerf::with_role(Role::CLIENT, 0.0, slow_ms),
            epoch: Instant::now(),
            cpu_ms: Vec::new(),
            gpu_ms: Vec::new(),
            window: None,
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
        detail: Option<SlowDetail>,
        world: impl FnOnce() -> WorldCounts,
    ) {
        self.cpu_ms.push(cpu_ms);
        self.gpu_ms.extend(gpu_ms);
        if self.perf.gauges_due(t) {
            self.perf
                .set_frame_medians(median(&mut self.cpu_ms), median(&mut self.gpu_ms));
        }
        self.perf.tick(t, wall_ms, detail, world);
        if let Some((end, reason)) = self.window
            && t >= end
        {
            self.stop(t, reason);
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
        };
        let path = self.perf.start_capture(t, dir, capture)?;
        self.window = Some(match options.seconds {
            Some(s) => (t + f64::from(s), StopReason::Normal),
            None => (t + MAX_WINDOW_S, StopReason::WindowCap),
        });
        Ok(path)
    }

    /// Ends the running capture; its file is finished off this thread and comes back from [`ClientPerf::take_saved`].
    pub fn stop(&mut self, t: f64, reason: StopReason) -> bool {
        self.window = None;
        self.perf.begin_stop(t, reason)
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

    pub fn status(&self, t: f64) -> Option<CaptureStatus> {
        self.perf.status(t)
    }

    /// The seconds left in the running capture's window.
    pub fn remaining_s(&self, t: f64) -> Option<f64> {
        self.window.map(|(end, _)| (end - t).max(0.0))
    }
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
    let mut machine: Machine = server_perf::machine(Some(server_perf::hardware_now()));
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
