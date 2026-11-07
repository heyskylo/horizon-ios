//! A dedicated server's .hzperf capture (docs/perf-platform.md §3, §4.1) and the always-on health history and slow ring behind it.
use crate::spans::Span;
use horizon_perf_format::files;
use horizon_perf_format::v1::{Kind, Options, Side, Start, Stop, StopReason, Trigger, Unit};
use horizon_perf_format::{
    CounterReporter, Field, FieldSender, HealthRecorder, Interner, SlowRecord, SlowRing, SlowSpan,
    WriterThread, counter_registry,
};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests;

mod system;
use system::SystemProbe;
pub use system::{build, hardware, machine, settings, warm_hardware};

/// What a slow tick spent its time on, gathered by the tick loop only when the tick was slow.
pub struct SlowDetail {
    pub cpu_ms: Option<f32>,
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

    /// One server tick that took `wall_ms` and ended at `t`; `world` is read when the gauges are due.
    pub fn tick(
        &mut self,
        t: f64,
        wall_ms: f32,
        detail: Option<SlowDetail>,
        world: impl FnOnce() -> WorldCounts,
    ) {
        if t - self.gauges_at >= 1.0 {
            self.gauges_at = t;
            let counts = world();
            let g = self.system.read(t, counts);
            self.health.set_gauges(g);
        }
        self.ticks += 1;
        self.health.record(t, wall_ms);
        if wall_ms > self.slow.threshold_ms() {
            let detail = detail.unwrap_or(SlowDetail {
                cpu_ms: None,
                spans: Vec::new(),
            });
            let record = SlowRecord {
                t,
                unit: Unit::Tick,
                counter: self.ticks,
                wall_ms,
                cpu_ms: detail.cpu_ms.unwrap_or(0.0),
                top: detail
                    .spans
                    .iter()
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
            session.pump(&self.health, t);
        }
    }

    /// Opens `dir/profile-<time>Z.hzperf` and writes what the server knew before: `Start`, the health tiers and the slow ring.
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
        };
        let mut start = options.start;
        start.side = Side::Server as i32;
        start.kind = Kind::Dedicated as i32;
        start.trigger = options.trigger as i32;
        start.started_unix_ms = now_ms;
        start.uptime_s = t - self.base;
        let window = Some(options.window_s);
        start.options = Some(Options {
            window_s: window,
            ..start.options.unwrap_or_default()
        });
        session.tx.send(Field::Start(start));
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

    /// Ends the running capture: the seconds closed so far, a last counters sample and `Stop`; None when none runs.
    pub fn stop_capture(&mut self, t: f64, reason: StopReason) -> Option<std::io::Result<Saved>> {
        let mut session = self.session.take()?;
        self.health.advance(t);
        session.pump(&self.health, t);
        session.tx.send(Field::Stop(Stop {
            side: Side::Server as i32,
            t: t - session.origin,
            reason: reason as i32,
            overhead: None,
        }));
        Some(session.finish())
    }
}

impl Session {
    fn entries(&mut self) {
        for entry in self.strings.take_new(Side::Server) {
            self.tx.send(Field::String(entry));
        }
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

    fn finish(self) -> std::io::Result<Saved> {
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
