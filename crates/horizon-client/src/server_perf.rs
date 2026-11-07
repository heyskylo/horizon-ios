//! A dedicated server's .hzperf capture (docs/perf-platform.md §3, §4.1) and the always-on health history and slow ring behind it.
use crate::spans::Span;
use horizon_perf_format::files;
use horizon_perf_format::v1::{
    Kind, Options, Overhead, Side, Start, Stop, StopReason, ThreadSelection, Trigger, Unit,
};
use horizon_perf_format::{
    CounterReporter, Field, FieldSender, HealthRecorder, Interner, SlowRecord, SlowRing, SlowSpan,
    WriterThread, counter_registry,
};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests;

mod samples;
pub mod spans;
mod system;
pub mod traffic;
pub mod world;
use samples::{Sampling, Stopped};
use spans::{SpanRecorder, Tick};
use system::SystemProbe;
pub use system::{build, game_rules, hardware, horizon_options, machine, settings, warm_hardware};
use traffic::{TrafficRecorder, Wire};
use world::{LevelCounts, MobCosts, WorkerJobs};

/// The thread whose ticks the span recorder sums: vanilla's name for it.
const SERVER_THREAD: &str = "Server thread";

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

/// Switches of one capture.
pub struct CaptureOptions {
    pub window_s: f64,
    pub trigger: Trigger,
    /// Settings, machine and build for `Start`; the capture fills the rest.
    pub start: Start,
    /// Whether the tick thread is sampled (where the platform can).
    pub samples: bool,
    /// Block names by id, for the samples' contexts.
    pub block_names: Vec<String>,
    /// The levels as the capture begins.
    pub levels: Vec<LevelCounts>,
    /// The connections as the capture begins.
    pub wire: Wire,
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
}

/// A finished capture: where it is and the older files retention removed.
pub struct Saved {
    pub path: PathBuf,
    pub deleted: Vec<PathBuf>,
}

struct Session {
    writer: WriterThread<File>,
    tx: FieldSender,
    path: PathBuf,
    dir: PathBuf,
    /// The recorder's clock reading when the capture began.
    origin: f64,
    cursor: u64,
    strings: Interner,
    counters: CounterReporter,
    sampling: Option<Sampling>,
    block_names: Vec<String>,
    spans: SpanRecorder,
    traffic: TrafficRecorder,
}

enum Ending {
    Running(JoinHandle<std::io::Result<Saved>>),
    Done(std::io::Result<Saved>),
}

/// The server's health tiers and slow ring, always on, and the capture running from them, if any.
pub struct ServerPerf {
    base: f64,
    health: HealthRecorder,
    slow: SlowRing,
    ticks: u64,
    gauges_at: f64,
    system: SystemProbe,
    session: Option<Session>,
    ending: Vec<Ending>,
}

impl ServerPerf {
    /// `start` is the clock reading (seconds) every later time is on; `slow_ms` is the slow ring's threshold.
    pub fn new(start: f64, slow_ms: f32) -> Self {
        Self {
            base: start,
            health: HealthRecorder::new(Side::Server, start),
            slow: SlowRing::new(slow_ms),
            ticks: 0,
            gauges_at: start,
            system: SystemProbe::default(),
            session: None,
            ending: Vec::new(),
        }
    }

    pub fn slow_threshold_ms(&self) -> f32 {
        self.slow.threshold_ms()
    }

    pub fn capturing(&self) -> bool {
        self.session.is_some()
    }

    pub fn health(&self) -> &HealthRecorder {
        &self.health
    }

    /// One server tick that took `wall_ms` and ended at `t`; `world` and `wire` are read when the gauges are due.
    pub fn tick(
        &mut self,
        t: f64,
        wall_ms: f32,
        detail: Option<TickDetail>,
        world: impl FnOnce() -> WorldCounts,
        wire: impl FnOnce() -> Wire,
    ) {
        if t - self.gauges_at >= 1.0 {
            self.gauges_at = t;
            let counts = world();
            let g = self.system.read(t, counts);
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
                unit: Unit::Tick,
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
        let (file, path) = files::create(dir, now_ms.div_euclid(1000))?;
        let writer = WriterThread::spawn(file)?;
        let tx = writer.sender();
        let sampling = options
            .samples
            .then(|| Sampling::start(options.window_s, self.slow.threshold_ms()))
            .flatten();
        self.health.advance(t);
        let (_, pushed) = self.health.samples_since(u64::MAX, t);
        let partial = (t - self.base).fract() > 1e-9;
        let mut session = Session {
            writer,
            tx,
            path: path.clone(),
            dir: dir.to_owned(),
            origin: t,
            cursor: pushed + u64::from(partial),
            strings: Interner::default(),
            counters: CounterReporter::new(Side::Server),
            sampling,
            block_names: options.block_names,
            spans: SpanRecorder::new(SERVER_THREAD),
            traffic: TrafficRecorder::new(options.wire),
        };
        let mut start = options.start;
        start.side = Side::Server as i32;
        start.kind = Kind::Dedicated as i32;
        start.trigger = options.trigger as i32;
        start.started_unix_ms = now_ms;
        start.uptime_s = t - self.base;
        let window = Some(options.window_s);
        let sampled = session.sampling.is_some();
        let threads = if sampled {
            ThreadSelection::Main
        } else {
            ThreadSelection::Unspecified
        };
        start.options = Some(Options {
            window_s: window,
            hz: if sampled { Sampling::hz() } else { 0 },
            threads: threads as i32,
            samples: sampled,
            ..start.options.unwrap_or_default()
        });
        if sampled && let Some(build) = &mut start.build {
            build.symbols = Sampling::symbols() as i32;
        }
        session.tx.send(Field::Start(start));
        let world = world::snapshot("start", 0.0, &options.levels, None, &mut session.strings);
        session.entries();
        session.tx.send(Field::World(world));
        for event in self.slow.snapshot(Side::Server, t, &mut session.strings) {
            session.entries();
            session.tx.send(Field::SlowHistory(event));
        }
        session.entries();
        for window in self.health.history(t) {
            session.tx.send(Field::History(window));
        }
        self.session = Some(session);
        Ok(path)
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
        let ending = match session.sampling.take().and_then(Sampling::stop) {
            None => Ending::Done(session.finish(None, stop_t, reason, overhead)),
            Some(stopped) => {
                let worker = thread::Builder::new()
                    .name("horizon-perf-finish".into())
                    .spawn(move || session.finish(Some(stopped), stop_t, reason, overhead));
                match worker {
                    Ok(handle) => Ending::Running(handle),
                    Err(e) => Ending::Done(Err(e)),
                }
            }
        };
        self.ending.push(ending);
        true
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
        for entry in self.strings.take_new(Side::Server) {
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
        let overhead = overhead(&self.spans, &end);
        self.traffic.sample(rel, &end.wire);
        let (last_block, summary) = std::mem::take(&mut self.spans).finish(&mut self.strings);
        let traffic = std::mem::take(&mut self.traffic).finish(rel);
        let workers = world::workers(&end.jobs, rel, &mut self.strings);
        let world = world::snapshot(
            "stop",
            rel,
            &end.levels,
            Some(&end.mob_costs),
            &mut self.strings,
        );
        self.entries();
        if let Some(block) = last_block {
            self.tx.send(Field::SpanBlock(block));
        }
        self.tx.send(Field::Spans(summary));
        self.tx.send(Field::Workers(workers));
        self.tx.send(Field::Net(traffic));
        self.tx.send(Field::World(world));
        overhead
    }

    fn slow(&mut self, record: &SlowRecord) {
        let event = record.to_proto(Side::Server, self.origin, &mut self.strings);
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
            let done = stopped.finish(&self.block_names, &mut self.strings);
            self.entries();
            self.tx.send(Field::Samples(done.samples));
            overhead = Overhead {
                spans_ms_per_tick: overhead.spans_ms_per_tick,
                spans_share: overhead.spans_share,
                mob_timing_ms_per_tick: overhead.mob_timing_ms_per_tick,
                mob_timing_share: overhead.mob_timing_share,
                ..done.overhead
            };
        }
        self.tx.send(Field::Stop(Stop {
            side: Side::Server as i32,
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
        writer.finish()?;
        let deleted = files::retain(&dir, &path, files::KEEP_FILES, files::KEEP_BYTES)?;
        Ok(Saved { path, deleted })
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
