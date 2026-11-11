//! A side's .hzperf capture (docs/perf-platform.md §3, §4.1), the dedicated server's or the client's, and the always-on health history and slow ring behind it.
use crate::gfx::pack_timing::{GpuReadout, TimedFrames};
use crate::spans::Span;
use horizon_client_render::perf_gpu;
use horizon_perf_format::files;
use horizon_perf_format::v1::{
    BenchResult, GpuMode, Kind, Mark, MemorySnapshot, Options, Overhead, Side, Start, Stop,
    StopReason, ThreadSelection, Trigger, Unit, Warning, WarningCode,
};
use horizon_perf_format::{
    CounterReporter, Field, FieldSender, Gauges, HealthRecorder, Interner, SlowRecord, SlowRing,
    SlowSpan, WriterThread, counter_registry,
};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests;

/// Tests that sample threads take turns: a sampler session arms every thread of the name it asks for, the other tests' too.
#[cfg(all(test, target_os = "linux"))]
pub(crate) static SAMPLED_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod samples;
pub mod spans;
mod system;
pub mod timeline;
pub mod traffic;
pub mod world;
pub use samples::MAX_WINDOW_S;
use samples::{Sampling, Stopped};
use spans::{SpanRecorder, Tick};
use system::{BackgroundProbe, SystemProbe};
pub use system::{
    build, game_rules, hardware, hardware_now, horizon_options, machine, settings, warm_hardware,
};
use timeline::{Drained, TimelineRecorder};
use traffic::{TrafficRecorder, Wire};
use world::{LevelCounts, MobCosts, WorkerJobs};

/// What a capturing side is: which side of the file it writes and the unit and sampled thread it counts in.
#[derive(Clone, Copy)]
pub struct Role {
    pub side: Side,
    pub kind: Kind,
    pub unit: Unit,
    pub thread: &'static str,
    /// The thread whose units the span recorder sums.
    pub span_thread: &'static str,
    /// Whether the process and machine probes run off the thread that feeds `tick`.
    pub background_probe: bool,
}

impl Role {
    pub const DEDICATED: Role = Role {
        side: Side::Server,
        kind: Kind::Dedicated,
        unit: Unit::Tick,
        thread: "tick",
        span_thread: "Server thread",
        background_probe: false,
    };
    pub const INTEGRATED: Role = Role {
        side: Side::Server,
        kind: Kind::Integrated,
        unit: Unit::Tick,
        thread: "tick",
        span_thread: "Server thread",
        background_probe: true,
    };
    pub const CLIENT: Role = Role {
        side: Side::Client,
        kind: Kind::Client,
        unit: Unit::Frame,
        thread: "render",
        span_thread: "Render thread",
        background_probe: true,
    };
}

/// What a tick spent its time on, gathered by the tick loop only when the tick was slow or a capture runs.
pub struct TickDetail {
    pub cpu_ms: Option<f32>,
    /// As recorded in the tick, each span after the spans inside it.
    pub spans: Vec<Span>,
}

/// The counts the health samples carry, read once a second.
#[derive(Clone, Copy, Default)]
pub struct WorldCounts {
    pub players: u32,
    pub chunks: u32,
    pub entities: u32,
}

/// The sampler's switches (`profile start --hz --over --wall --threads`); the default is the sampler's own.
#[derive(Clone, Copy, Default)]
pub struct SamplerOptions {
    pub hz: Option<u32>,
    /// Only ticks or frames at least this slow are sampled; the slow ring's threshold when unset.
    pub over_ms: Option<f32>,
    pub wall: bool,
    /// Every thread that opted in, not just the side's main thread.
    pub all_threads: bool,
}

/// Switches of one capture.
pub struct CaptureOptions {
    /// Unset when the capture runs until stopped, which the file records and the longest window ends.
    pub window_s: Option<f64>,
    pub trigger: Trigger,
    /// Settings, machine and build for `Start`; the capture fills the rest.
    pub start: Start,
    /// Whether the side's main thread is sampled (where the platform can).
    pub samples: bool,
    pub sampler: SamplerOptions,
    /// Block names by id, for the samples' contexts.
    pub block_names: Vec<String>,
    /// The levels as the capture begins.
    pub levels: Vec<LevelCounts>,
    /// The connections as the capture begins.
    pub wire: Wire,
    /// Whether the GPU's pass timings are recorded: unspecified on a side without a GPU.
    pub gpu: GpuMode,
    /// Records every thread's spans as `timeline` chunks within these caps, unless another capture already does.
    pub timeline: Option<timeline::Caps>,
    /// Write the capture at exactly this path, kept by no retention, instead of into the folder given to `start_capture`.
    pub file: Option<PathBuf>,
}

/// What the game gathers as a capture ends.
#[derive(Default)]
pub struct CaptureEnd {
    pub levels: Vec<LevelCounts>,
    pub mob_costs: MobCosts,
    pub jobs: Vec<WorkerJobs>,
    pub wire: Wire,
    /// What timing one span and one mob costs on this machine, in nanoseconds.
    pub span_ns: f64,
    pub mob_ns: f64,
    /// The GPU pass timings, when the capture asked for them.
    pub gpu: Option<GpuReadout>,
    /// Benchmark reports to embed verbatim; the capture gives them its side.
    pub bench: Vec<BenchResult>,
}

/// A finished capture: where it is and the older files retention removed.
pub struct Saved {
    pub path: PathBuf,
    pub deleted: Vec<PathBuf>,
}

/// Where a running capture is, for `profile status`.
pub struct CaptureStatus {
    pub path: PathBuf,
    pub elapsed_s: f64,
    pub bytes: u64,
}

struct Session {
    side: Side,
    unit: Unit,
    /// Unset when the session joined another side's file, which that side finishes.
    writer: Option<WriterThread<File>>,
    tx: FieldSender,
    path: PathBuf,
    /// The folder retention trims; none for a capture written to a path of its own.
    dir: Option<PathBuf>,
    /// The recorder's clock reading when the capture began.
    origin: f64,
    cursor: u64,
    strings: Interner,
    counters: CounterReporter,
    sampling: Option<Sampling>,
    block_names: Vec<String>,
    spans: SpanRecorder,
    traffic: TrafficRecorder,
    gpu: GpuMode,
    /// The GPU timers' frames and the window's CPU frame times, and when their last block was written.
    gpu_log: perf_gpu::FrameLog,
    gpu_block_t: f64,
    timeline: Option<TimelineRecorder>,
}

enum Ending {
    Running(JoinHandle<std::io::Result<Saved>>),
    Done(std::io::Result<Saved>),
}

/// The server's health tiers and slow ring, always on, and the capture running from them, if any.
pub struct ServerPerf {
    role: Role,
    base: f64,
    health: HealthRecorder,
    slow: SlowRing,
    ticks: u64,
    gauges_at: f64,
    system: SystemProbe,
    background: Option<BackgroundProbe>,
    frame_medians: (Option<f32>, Option<f32>),
    session: Option<Session>,
    ending: Vec<Ending>,
}

impl ServerPerf {
    /// `start` is the clock reading (seconds) every later time is on; `slow_ms` is the slow ring's threshold.
    pub fn new(start: f64, slow_ms: f32) -> Self {
        Self::with_role(Role::DEDICATED, start, slow_ms)
    }

    pub fn with_role(role: Role, start: f64, slow_ms: f32) -> Self {
        Self {
            role,
            base: start,
            health: HealthRecorder::new(role.side, start),
            slow: SlowRing::new(slow_ms),
            ticks: 0,
            gauges_at: start,
            system: SystemProbe::default(),
            background: None,
            frame_medians: (None, None),
            session: None,
            ending: Vec::new(),
        }
    }

    pub fn slow_threshold_ms(&self) -> f32 {
        self.slow.threshold_ms()
    }

    /// The slow ring's ticks or frames, oldest first.
    pub fn slow_records(&self) -> impl Iterator<Item = &SlowRecord> {
        self.slow.records()
    }

    pub fn capturing(&self) -> bool {
        self.session.is_some()
    }

    pub fn health(&self) -> &HealthRecorder {
        &self.health
    }

    /// Whether the next `tick` reads the gauges, so a client can set the frame medians of the second first.
    pub fn gauges_due(&self, t: f64) -> bool {
        t - self.gauges_at >= 1.0
    }

    /// The median CPU and GPU time of the frames since the last second, for the gauges read next.
    pub fn set_frame_medians(&mut self, cpu_ms: Option<f32>, gpu_ms: Option<f32>) {
        self.frame_medians = (cpu_ms, gpu_ms);
    }

    /// One tick or frame that took `wall_ms` and ended at `t`; `world` and `wire` are read when the gauges are due.
    pub fn tick(
        &mut self,
        t: f64,
        wall_ms: f32,
        detail: Option<TickDetail>,
        world: impl FnOnce() -> WorldCounts,
        wire: impl FnOnce() -> Wire,
    ) {
        let first_background = self.role.background_probe && self.background.is_none();
        let due = self.gauges_due(t);
        let counts = (first_background || due).then(world);
        if let (true, Some(counts)) = (first_background, counts) {
            let probe = BackgroundProbe::spawn();
            probe.request(t, counts);
            self.background = Some(probe);
        }
        if let (true, Some(counts)) = (due, counts) {
            self.gauges_at = t;
            let mut g = match &self.background {
                Some(probe) => {
                    probe.request(t, counts);
                    Gauges {
                        players: counts.players,
                        chunks: counts.chunks,
                        entities: counts.entities,
                        ..probe.latest()
                    }
                }
                None => self.system.read(t, counts),
            };
            (g.frame_cpu_ms_p50, g.frame_gpu_ms_p50) = self.frame_medians;
            self.health.set_gauges(g);
            if let Some(session) = &mut self.session {
                session.traffic.sample(t - session.origin, &wire());
            }
        }
        self.ticks += 1;
        self.health.record(t, wall_ms);
        if wall_ms > self.slow.threshold_ms() {
            let record = SlowRecord {
                t,
                unit: self.role.unit,
                counter: self.ticks,
                wall_ms,
                cpu_ms: detail.as_ref().and_then(|d| d.cpu_ms).unwrap_or(0.0),
                top: detail
                    .iter()
                    .flat_map(|d| crate::spans::merge(&d.spans))
                    .map(|s| SlowSpan {
                        name: s.name.to_owned(),
                        wall_ms: s.ms,
                        cpu_ms: s.cpu_ms,
                        depth: u32::from(s.depth),
                    })
                    .collect(),
            };
            if self.slow.offer(record)
                && let Some(session) = &mut self.session
                && let Some(kept) = self.slow.records().last()
            {
                session.slow(kept);
            }
        }
        if let Some(session) = &mut self.session {
            session.span_tick(t, self.ticks, wall_ms, detail.as_ref());
            session.pump(&self.health, t);
            session.timeline_tick(t);
        }
    }

    /// The window's frame that ended at `t`: its CPU ms and the GPU frames timed since the last, written about once a second when the capture times the GPU.
    pub fn gpu_frames(&mut self, t: f64, cpu_ms: f32, frames: &TimedFrames) {
        let Some(session) = &mut self.session else {
            return;
        };
        if matches!(session.gpu, GpuMode::Unspecified | GpuMode::Off) {
            return;
        }
        let (now, origin) = (Instant::now(), session.origin);
        session.gpu_log.cpu_frame(cpu_ms);
        session.gpu_log.add(
            frames,
            |at| t - now.saturating_duration_since(at).as_secs_f64() - origin,
            &mut session.strings,
        );
        if t - origin - session.gpu_block_t >= 1.0 {
            session.gpu_block_t = t - origin;
            session.gpu_block();
        }
    }

    /// Opens `dir/profile-<time>Z.hzperf` with `Start`, the health tiers and the slow ring; call it on the tick thread, which it samples.
    pub fn start_capture(
        &mut self,
        t: f64,
        dir: &Path,
        options: CaptureOptions,
    ) -> std::io::Result<PathBuf> {
        if self.session.is_some() {
            return Err(std::io::Error::other("a capture is already running"));
        }
        let now_ms = unix_ms();
        let (file, path) = match &options.file {
            Some(path) => (files::create_at(path)?, path.clone()),
            None => files::create(dir, now_ms.div_euclid(1000))?,
        };
        let writer = WriterThread::spawn_with(file, profiler_thread)?;
        let tx = writer.sender();
        self.open(t, t, Some(writer), tx, path.clone(), dir, options);
        Ok(path)
    }

    /// Writes this side into the file another side opened; `origin`, on this side's clock, is the zero of every `t`. Call it on the tick thread.
    pub fn join_capture(
        &mut self,
        t: f64,
        origin: f64,
        tx: FieldSender,
        path: PathBuf,
        options: CaptureOptions,
    ) -> std::io::Result<()> {
        if self.session.is_some() {
            return Err(std::io::Error::other("a capture is already running"));
        }
        self.open(t, origin, None, tx, path, Path::new(""), options);
        Ok(())
    }

    /// What another side needs to join the running capture: its sender, its file and the clock reading its `t` is zero at.
    pub fn joinable(&self) -> Option<(FieldSender, PathBuf, f64)> {
        let s = self.session.as_ref()?;
        Some((s.tx.clone(), s.path.clone(), s.origin))
    }

    #[allow(clippy::too_many_arguments)]
    fn open(
        &mut self,
        t: f64,
        origin: f64,
        writer: Option<WriterThread<File>>,
        tx: FieldSender,
        path: PathBuf,
        dir: &Path,
        options: CaptureOptions,
    ) {
        let now_ms = unix_ms();
        let sampling = options
            .samples
            .then(|| {
                Sampling::start(
                    self.role.thread,
                    options.window_s.unwrap_or(MAX_WINDOW_S),
                    options.sampler.over_ms.unwrap_or(self.slow.threshold_ms()),
                    &options.sampler,
                )
            })
            .flatten();
        self.health.advance(t);
        let (_, pushed) = self.health.samples_since(u64::MAX, t);
        let partial = (t - self.base).fract() > 1e-9;
        let timeline = options
            .timeline
            .and_then(|caps| TimelineRecorder::start(self.role.side, caps));
        let mut session = Session {
            side: self.role.side,
            unit: self.role.unit,
            writer,
            tx,
            path,
            dir: options.file.is_none().then(|| dir.to_owned()),
            origin,
            cursor: pushed + u64::from(partial),
            strings: Interner::default(),
            counters: CounterReporter::new(self.role.side),
            sampling,
            block_names: options.block_names,
            spans: SpanRecorder::new(self.role.span_thread, self.role.side, self.role.unit),
            traffic: TrafficRecorder::new(options.wire),
            gpu: options.gpu,
            gpu_log: perf_gpu::FrameLog::default(),
            gpu_block_t: 0.0,
            timeline,
        };
        let mut start = options.start;
        if let Some((used, total)) = system::disk_mb() {
            start.world_fs_used_mb = used;
            start.world_fs_total_mb = total;
        }
        start.side = self.role.side as i32;
        start.kind = self.role.kind as i32;
        start.trigger = options.trigger as i32;
        start.started_unix_ms = now_ms;
        start.uptime_s = t - self.base;
        let sampled = session.sampling.is_some();
        let threads = match (sampled, options.sampler.all_threads) {
            (false, _) => ThreadSelection::Unspecified,
            (true, false) => ThreadSelection::Main,
            (true, true) => ThreadSelection::All,
        };
        start.options = Some(Options {
            window_s: options.window_s,
            hz: if sampled {
                Sampling::hz(&options.sampler)
            } else {
                0
            },
            threads: threads as i32,
            ticks_over_ms: options.sampler.over_ms.filter(|_| sampled),
            wall: sampled && options.sampler.wall,
            samples: sampled,
            gpu: options.gpu as i32,
            timeline: session.timeline.is_some(),
        });
        if sampled && let Some(build) = &mut start.build {
            build.symbols = Sampling::symbols() as i32;
        }
        session.tx.send(Field::Start(start));
        let mut world = world::snapshot("start", 0.0, &options.levels, None, &mut session.strings);
        world.side = self.role.side as i32;
        session.entries();
        session.tx.send(Field::World(world));
        for event in self
            .slow
            .snapshot(self.role.side, origin, &mut session.strings)
        {
            session.entries();
            session.tx.send(Field::SlowHistory(event));
        }
        session.entries();
        for window in self.health.history(origin) {
            session.tx.send(Field::History(window));
        }
        self.session = Some(session);
    }

    /// Ends the running capture: writes the spans, workers, net and world here and finishes the file off the tick thread; false when none runs.
    pub fn begin_stop(&mut self, t: f64, reason: StopReason, end: CaptureEnd) -> bool {
        let Some(mut session) = self.session.take() else {
            return false;
        };
        self.health.advance(t);
        session.pump(&self.health, t);
        let stop_t = t - session.origin;
        let overhead = session.close(stop_t, end);
        let stopped = session.sampling.take().and_then(Sampling::stop);
        // The file's owner waits for the sides that joined it, so it never finishes on the caller's thread.
        let ending = if stopped.is_none() && session.writer.is_none() {
            Ending::Done(session.finish(None, stop_t, reason, overhead))
        } else {
            let worker = thread::Builder::new()
                .name("horizon-perf-finish".into())
                .spawn(move || {
                    profiler_thread();
                    session.finish(stopped, stop_t, reason, overhead)
                });
            match worker {
                Ok(handle) => Ending::Running(handle),
                Err(e) => Ending::Done(Err(e)),
            }
        };
        self.ending.push(ending);
        true
    }

    /// Whether a stopped capture's file is still being finished or waits in [`ServerPerf::take_saved`].
    pub fn finishing(&self) -> bool {
        !self.ending.is_empty()
    }

    /// The captures whose files are done, oldest first; with `wait`, every one that was begun.
    pub fn take_saved(&mut self, wait: bool) -> Vec<std::io::Result<Saved>> {
        let mut saved = Vec::new();
        for ending in std::mem::take(&mut self.ending) {
            match ending {
                Ending::Done(result) => saved.push(result),
                Ending::Running(handle) if wait || handle.is_finished() => {
                    saved.push(handle.join().unwrap_or_else(|_| {
                        Err(std::io::Error::other(
                            "the capture's finishing thread panicked",
                        ))
                    }))
                }
                running => self.ending.push(running),
            }
        }
        saved
    }

    /// Types `name` into the running capture at `t`; false when none runs.
    pub fn mark(&mut self, t: f64, name: &str) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        session.tx.send(Field::Mark(Mark {
            side: session.side as i32,
            t: t - session.origin,
            name: name.to_owned(),
        }));
        true
    }

    /// Records a memory snapshot, rows as flattened paths and values, in the running capture; false when none runs.
    pub fn memory_snapshot(&mut self, t: f64, label: &str, rows: &[(String, f64)]) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        let label = session.strings.intern(label);
        let path = rows
            .iter()
            .map(|(name, _)| session.strings.intern(name))
            .collect();
        session.entries();
        session.tx.send(Field::Memory(MemorySnapshot {
            side: session.side as i32,
            t: t - session.origin,
            label,
            path,
            value_mb: rows.iter().map(|(_, value)| *value).collect(),
        }));
        true
    }

    /// Records `code` at `t` in the running capture; false when none runs.
    pub fn warn(&mut self, t: f64, code: WarningCode) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        session.tx.send(Field::Warning(Warning {
            side: session.side as i32,
            t: t - session.origin,
            code: code as i32,
            args: Vec::new(),
        }));
        true
    }

    /// The running capture's file, age and size so far; None when none runs.
    pub fn status(&self, t: f64) -> Option<CaptureStatus> {
        let session = self.session.as_ref()?;
        Some(CaptureStatus {
            path: session.path.clone(),
            elapsed_s: t - session.origin,
            bytes: std::fs::metadata(&session.path).map_or(0, |m| m.len()),
        })
    }

    /// Ends the running capture and waits for its file; None when none runs.
    pub fn stop_capture(
        &mut self,
        t: f64,
        reason: StopReason,
        end: CaptureEnd,
    ) -> Option<std::io::Result<Saved>> {
        self.begin_stop(t, reason, end).then(|| {
            self.take_saved(true)
                .pop()
                .unwrap_or_else(|| Err(std::io::Error::other("the capture vanished")))
        })
    }
}

/// What recording cost the tick, priced at the end of the capture: every span two clocks and two thread-CPU clocks, every sampled mob two clocks.
fn overhead(spans: &SpanRecorder, end: &CaptureEnd) -> Overhead {
    let ticks = spans.count().max(1) as f64;
    let mspt = spans.mean_wall_ms().max(1e-9);
    let spans_ms = spans.spans_recorded() as f64 / ticks * end.span_ns / 1e6;
    let mob_samples: u64 = end.mob_costs.costs.iter().map(|c| c.mob_ticks).sum();
    let mob_ms = mob_samples as f64 / ticks * end.mob_ns / 1e6;
    Overhead {
        spans_ms_per_tick: spans_ms as f32,
        spans_share: (spans_ms / mspt) as f32,
        mob_timing_ms_per_tick: mob_ms as f32,
        mob_timing_share: (mob_ms / mspt) as f32,
        ..Overhead::default()
    }
}

impl Session {
    fn entries(&mut self) {
        for entry in self.strings.take_new(self.side) {
            self.tx.send(Field::String(entry));
        }
    }

    fn span_tick(&mut self, t: f64, counter: u64, wall_ms: f32, detail: Option<&TickDetail>) {
        let tick = Tick {
            t: t - self.origin,
            counter,
            wall_ms,
            cpu_ms: detail.and_then(|d| d.cpu_ms),
            spans: detail.map_or(&[], |d| &d.spans),
        };
        if let Some(block) = self.spans.unit(tick, &mut self.strings) {
            self.entries();
            self.tx.send(Field::SpanBlock(block));
        }
    }

    /// Writes the spans, workers, net and world as the capture ends; returns what recording cost.
    fn close(&mut self, rel: f64, end: CaptureEnd) -> Overhead {
        let mut overhead = overhead(&self.spans, &end);
        if let Some(timeline) = self.timeline.take() {
            let (drained, stopped_t) = timeline.finish(rel, &mut self.strings);
            self.send_timeline(drained);
            overhead.timeline_stopped_t = stopped_t;
        }
        self.traffic.sample(rel, &end.wire);
        let (last_block, summary) = std::mem::take(&mut self.spans).finish(&mut self.strings);
        let mut traffic = std::mem::take(&mut self.traffic).finish(rel);
        traffic.side = self.side as i32;
        let mut workers = world::workers(&end.jobs, rel, &mut self.strings);
        workers.side = self.side as i32;
        let mut world = world::snapshot(
            "stop",
            rel,
            &end.levels,
            Some(&end.mob_costs),
            &mut self.strings,
        );
        world.side = self.side as i32;
        self.entries();
        if let Some(block) = last_block {
            self.tx.send(Field::SpanBlock(block));
        }
        self.tx.send(Field::Spans(summary));
        self.tx.send(Field::Workers(workers));
        self.tx.send(Field::Net(traffic));
        self.gpu(rel, end.gpu.as_ref());
        self.tx.send(Field::World(world));
        for mut bench in end.bench {
            bench.side = self.side as i32;
            self.tx.send(Field::Bench(bench));
        }
        overhead
    }

    /// Writes the GPU timings the capture asked for, or a warning when there were none.
    fn gpu(&mut self, rel: f64, readout: Option<&GpuReadout>) {
        if matches!(self.gpu, GpuMode::Unspecified | GpuMode::Off) {
            return;
        }
        self.gpu_block();
        let sections = readout.map_or_else(Vec::new, |r| {
            perf_gpu::gpu_passes(r, &self.gpu_log, self.side, &mut self.strings)
        });
        self.entries();
        if sections.is_empty() {
            self.tx.send(Field::Warning(Warning {
                side: self.side as i32,
                t: rel,
                code: WarningCode::GpuUnavailable as i32,
                args: Vec::new(),
            }));
        }
        for gpu in sections {
            self.tx.send(Field::Gpu(gpu));
        }
    }

    fn gpu_block(&mut self) {
        if let Some(block) = self.gpu_log.take_block(self.side) {
            self.entries();
            self.tx.send(Field::GpuFrames(block));
        }
    }

    fn timeline_tick(&mut self, t: f64) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        if let Some(drained) = timeline.tick(t - self.origin, &mut self.strings) {
            self.send_timeline(drained);
        }
    }

    fn send_timeline(&mut self, drained: Drained) {
        self.entries();
        for chunk in drained.chunks {
            self.tx.send(Field::Timeline(chunk));
        }
        if let Some(warning) = drained.warning {
            self.tx.send(Field::Warning(warning));
        }
    }

    fn slow(&mut self, record: &SlowRecord) {
        let event = record.to_proto(self.side, self.origin, &mut self.strings);
        self.entries();
        self.tx.send(Field::Slow(event));
    }

    /// Appends the seconds closed since the last call, and counters with them.
    fn pump(&mut self, health: &HealthRecorder, t: f64) {
        let (samples, next) = health.samples_since(self.cursor, self.origin);
        self.cursor = self.cursor.max(next);
        if samples.is_empty() {
            return;
        }
        for sample in samples {
            self.tx.send(Field::Health(sample));
        }
        self.counters_now(t);
    }

    fn counters_now(&mut self, t: f64) {
        let registry = counter_registry();
        if registry.is_empty() {
            return;
        }
        let report = self
            .counters
            .report(registry, &mut self.strings, t - self.origin);
        self.entries();
        for def in report.defs {
            self.tx.send(Field::CounterDef(def));
        }
        self.tx.send(Field::Counters(report.sample));
    }

    /// Writes the samples (when there are any) and `Stop`, closes the file and applies retention.
    fn finish(
        mut self,
        stopped: Option<Stopped>,
        stop_t: f64,
        reason: StopReason,
        mut overhead: Overhead,
    ) -> std::io::Result<Saved> {
        if let Some(stopped) = stopped {
            let done = stopped.finish(self.side, self.unit, &self.block_names, &mut self.strings);
            self.entries();
            self.tx.send(Field::Samples(done.samples));
            overhead = Overhead {
                spans_ms_per_tick: overhead.spans_ms_per_tick,
                spans_share: overhead.spans_share,
                mob_timing_ms_per_tick: overhead.mob_timing_ms_per_tick,
                mob_timing_share: overhead.mob_timing_share,
                timeline_stopped_t: overhead.timeline_stopped_t,
                ..done.overhead
            };
        }
        self.tx.send(Field::Stop(Stop {
            side: self.side as i32,
            t: stop_t,
            reason: reason as i32,
            overhead: Some(overhead),
        }));
        let Session {
            writer,
            tx,
            path,
            dir,
            ..
        } = self;
        drop(tx);
        let Some(writer) = writer else {
            return Ok(Saved {
                path,
                deleted: Vec::new(),
            });
        };
        writer.finish()?;
        let deleted = match dir {
            Some(dir) => files::retain(&dir, &path, files::KEEP_FILES, files::KEEP_BYTES)?,
            None => Vec::new(),
        };
        Ok(Saved { path, deleted })
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Keeps the calling thread's allocations out of the heap samples: it is one of the profiler's own.
pub(crate) fn profiler_thread() {
    #[cfg(target_os = "linux")]
    horizon_threads::heap_sample::ignore_this_thread();
}
