//! A profiling session: the sampler's rings drained by a writer thread into bounded aggregates, reported at stop.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use crate::sampler;

mod agg;
mod symbols;
mod tree;

pub use agg::{Aggregate, Frame, Key, MAX_TIME_BLOCKS, State, TickStats};
pub use symbols::{Sym, SymbolQuality, Symbols, symbol_quality, symbolize};
pub use tree::{FNode, Interner, ROOT, Resolved, Row, top_functions, weights};

const DRAIN_EVERY: Duration = Duration::from_millis(100);

/// Display names for the ids the context stores hold (0 is "none"); an id without a name shows as its number.
#[derive(Clone, Copy)]
pub struct Names {
    pub span: fn(u16) -> Option<String>,
    pub entity: fn(u16) -> Option<String>,
    pub block: fn(u16) -> Option<String>,
}

impl Default for Names {
    fn default() -> Self {
        Self {
            span: |_| None,
            entity: |_| None,
            block: |_| None,
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub sampler: sampler::Config,
    /// Sampled threads to profile by name; empty means every thread that opted in.
    pub threads: Vec<String>,
    /// Ticks (or frames) at least this long count as slow.
    pub ticks_over_ms: f64,
    /// Seconds per time block; a session longer than `MAX_TIME_BLOCKS` of them keeps the rest in the last block.
    pub block_s: f32,
    pub names: Names,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sampler: sampler::Config::default(),
            threads: Vec::new(),
            ticks_over_ms: 50.0,
            block_s: 1.0,
            names: Names::default(),
        }
    }
}

/// A snapshot of a running session, for a `status` command.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub elapsed: Duration,
    pub cpu_samples: u64,
    pub wall_samples: u64,
    pub ticks: u64,
    pub dropped: u64,
    pub nodes: usize,
}

#[derive(Default)]
struct Shared {
    cpu_samples: AtomicU64,
    wall_samples: AtomicU64,
    ticks: AtomicU64,
    dropped: AtomicU64,
    nodes: AtomicU64,
}

/// What a stopped session recorded.
pub struct Profile {
    pub agg: Aggregate,
    pub config: Config,
    pub stats: sampler::Stats,
    pub elapsed: Duration,
    pub started: SystemTime,
    /// CPU time the writer thread used (unwinding and aggregating), whole session.
    pub writer_cpu_ns: u64,
}

struct Finished {
    agg: Aggregate,
    stats: sampler::Stats,
    writer_cpu_ns: u64,
}

pub struct Session {
    config: Config,
    shared: Arc<Shared>,
    stop: mpsc::Sender<()>,
    writer: JoinHandle<Finished>,
    started: Instant,
    started_at: SystemTime,
}

/// Starts sampling the opted-in threads `config` selects, with a writer thread draining them.
pub fn start(config: Config) -> io::Result<Session> {
    let wanted = config.threads.clone();
    let mut session = sampler::start(config.sampler, |name| {
        wanted.is_empty() || wanted.iter().any(|w| w == name)
    })?;
    let shared = Arc::new(Shared::default());
    let slow_ns = (config.ticks_over_ms.max(0.0) * 1e6) as u64;
    let block = Duration::from_secs_f32(config.block_s.max(0.001));
    let begun = Instant::now();
    let (stop, stopped) = mpsc::channel::<()>();
    let publish = shared.clone();
    let writer = thread::Builder::new()
        .name("horizon-profile".into())
        .spawn(move || {
            let cpu0 = sampler::thread_cpu_ns();
            let mut agg = Aggregate::new(slow_ns);
            let in_block = |agg: &mut Aggregate| {
                agg.set_time_block((begun.elapsed().as_secs_f64() / block.as_secs_f64()) as usize)
            };
            while stopped.recv_timeout(DRAIN_EVERY) == Err(mpsc::RecvTimeoutError::Timeout) {
                in_block(&mut agg);
                session.drain(|name, record| agg.add(name, record));
                publish.cpu_samples.store(agg.cpu_samples, Relaxed);
                publish.wall_samples.store(agg.wall_samples, Relaxed);
                publish.ticks.store(agg.tick_markers, Relaxed);
                publish.dropped.store(session.stats().dropped, Relaxed);
                publish.nodes.store(agg.nodes() as u64, Relaxed);
            }
            in_block(&mut agg);
            let stats = session.stop(|name, record| agg.add(name, record));
            agg.finish();
            Finished {
                agg,
                stats,
                writer_cpu_ns: sampler::thread_cpu_ns().saturating_sub(cpu0),
            }
        })?;
    Ok(Session {
        config,
        shared,
        stop,
        writer,
        started: Instant::now(),
        started_at: SystemTime::now(),
    })
}

impl Session {
    pub fn status(&self) -> Status {
        let s = &self.shared;
        Status {
            elapsed: self.started.elapsed(),
            cpu_samples: s.cpu_samples.load(Relaxed),
            wall_samples: s.wall_samples.load(Relaxed),
            ticks: s.ticks.load(Relaxed),
            dropped: s.dropped.load(Relaxed),
            nodes: s.nodes.load(Relaxed) as usize,
        }
    }

    /// Cancels the timers, drains what is left and returns the aggregates.
    pub fn stop(self) -> io::Result<Profile> {
        let elapsed = self.started.elapsed();
        let _ = self.stop.send(());
        let done = self
            .writer
            .join()
            .map_err(|_| io::Error::other("the profile writer thread panicked"))?;
        Ok(Profile {
            agg: done.agg,
            config: self.config,
            stats: done.stats,
            elapsed,
            started: self.started_at,
            writer_cpu_ns: done.writer_cpu_ns,
        })
    }
}

#[cfg(test)]
mod tests;
