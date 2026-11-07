//! In-process stack sampling of named threads: per-thread rings filled by a signal handler, nothing running while off.

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize,
};
use std::sync::atomic::{Ordering::*, compiler_fence};
use std::sync::{Arc, Mutex, OnceLock};

use crate::unwind::{self, MAX_FRAMES, StackBounds};

mod linux;
use linux as os;

pub const DEFAULT_HZ: u32 = 250;
const TIME_EVERY: u32 = 64;

/// What the thread was doing when a sample hit, written with plain stores where the work changes hands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub tick: u64,
    pub span: u16,
    pub entity_type: u16,
    pub chunk: (i32, i32),
    pub block: u16,
    pub dim: u8,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cpu = 0,
    /// End of a tick or frame: `value` is its duration in nanoseconds, `ctx.tick` its number.
    Tick = 1,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Record {
    pub kind: Kind,
    pub depth: u8,
    pub ctx: Context,
    pub value: u64,
    pub frames: [usize; MAX_FRAMES],
}

impl Record {
    /// The interrupted pc, then return addresses, innermost first.
    pub fn stack(&self) -> &[usize] {
        &self.frames[..self.depth as usize]
    }
}

struct Cells {
    tick: AtomicU64,
    span: AtomicU16,
    entity: AtomicU16,
    chunk: (AtomicI32, AtomicI32),
    block: AtomicU16,
    dim: AtomicU8,
}

thread_local! {
    static CTX: Cells = const { Cells { tick: AtomicU64::new(0), span: AtomicU16::new(0), entity: AtomicU16::new(0),
        chunk: (AtomicI32::new(0), AtomicI32::new(0)), block: AtomicU16::new(0), dim: AtomicU8::new(0) } };
    static LOCAL: Cell<*const ThreadState> = const { Cell::new(std::ptr::null()) };
    static GUARD: RefCell<Option<Registered>> = const { RefCell::new(None) };
}

macro_rules! setter {
    ($name:ident, $field:ident, $t:ty) => {
        pub fn $name(v: $t) {
            CTX.with(|c| c.$field.store(v, Relaxed))
        }
    };
}
setter!(set_tick, tick, u64);
setter!(set_span, span, u16);
setter!(set_entity_type, entity, u16);
setter!(set_block, block, u16);
setter!(set_dim, dim, u8);

pub fn set_chunk(x: i32, z: i32) {
    CTX.with(|c| (c.chunk.0.store(x, Relaxed), c.chunk.1.store(z, Relaxed)));
}

pub fn context() -> Context {
    CTX.with(|c| Context {
        tick: c.tick.load(Relaxed),
        span: c.span.load(Relaxed),
        entity_type: c.entity.load(Relaxed),
        chunk: (c.chunk.0.load(Relaxed), c.chunk.1.load(Relaxed)),
        block: c.block.load(Relaxed),
        dim: c.dim.load(Relaxed),
    })
}

/// Single-producer, single-consumer ring of fixed slots; the producer is one thread (handler or marker, never both at once).
struct Ring {
    slots: *mut Record,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

// SAFETY: slots are written only by the owning thread before `head` is published and read only below `head`.
unsafe impl Send for Ring {}
// SAFETY: see Send; the indices are atomics.
unsafe impl Sync for Ring {}

impl Ring {
    fn new(slots: usize) -> io::Result<Self> {
        let cap = slots.max(2).next_power_of_two();
        let layout = Layout::array::<Record>(cap).map_err(io::Error::other)?;
        // SAFETY: the layout is non-zero sized; an all-zero Record is valid (Kind::Cpu is 0).
        let slots = unsafe { alloc_zeroed(layout) } as *mut Record;
        if slots.is_null() {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        Ok(Self {
            slots,
            mask: cap - 1,
            head: 0.into(),
            tail: 0.into(),
            dropped: 0.into(),
        })
    }

    fn reserve(&self) -> Option<*mut Record> {
        let head = self.head.load(Relaxed);
        if head.wrapping_sub(self.tail.load(Acquire)) > self.mask {
            self.dropped.fetch_add(1, Relaxed);
            return None;
        }
        // SAFETY: the index is masked into the allocation.
        Some(unsafe { self.slots.add(head & self.mask) })
    }

    fn publish(&self) {
        self.head
            .store(self.head.load(Relaxed).wrapping_add(1), Release);
    }

    fn clear(&self) {
        self.tail.store(self.head.load(Acquire), Release);
        self.dropped.store(0, Relaxed);
    }

    fn drain(&self, mut f: impl FnMut(&Record)) {
        let (tail, head) = (self.tail.load(Relaxed), self.head.load(Acquire));
        for i in tail..head {
            // SAFETY: slots below the acquired head were fully written before the producer published them.
            f(unsafe { &*self.slots.add(i & self.mask) });
        }
        self.tail.store(head, Release);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        // SAFETY: the same layout `new` allocated with.
        unsafe {
            dealloc(
                self.slots as *mut u8,
                Layout::array::<Record>(self.mask + 1).unwrap(),
            )
        }
    }
}

#[derive(Default)]
struct ThreadState {
    name: &'static str,
    target: os::Target,
    bounds: StackBounds,
    ring: OnceLock<Ring>,
    armed: AtomicBool,
    busy: AtomicBool,
    seen: AtomicU32,
    samples: AtomicU64,
    timed: AtomicU64,
    timed_ns: AtomicU64,
    timer: Mutex<Option<os::Timer>>,
}

impl ThreadState {
    fn disarm(&self) {
        self.armed.store(false, Release);
        if let Some(timer) = self.timer.lock().unwrap().take() {
            os::disarm(timer);
        }
    }
}

static REGISTRY: Mutex<Vec<Arc<ThreadState>>> = Mutex::new(Vec::new());

struct Registered(Arc<ThreadState>);

impl Drop for Registered {
    fn drop(&mut self) {
        let mut registry = REGISTRY.lock().unwrap();
        registry.retain(|s| !Arc::ptr_eq(s, &self.0));
        self.0.disarm();
        LOCAL.with(|l| l.set(std::ptr::null()));
    }
}

/// Opts the calling thread in to sampling under `name` (idempotent); false when its stack or the platform is unknown.
pub fn sampled_thread(name: &'static str) -> bool {
    GUARD.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_some() {
            return true;
        }
        let (Some(bounds), Some(target)) = (StackBounds::current(), os::Target::current()) else {
            return false;
        };
        let state = Arc::new(ThreadState {
            name,
            target,
            bounds,
            ..Default::default()
        });
        REGISTRY.lock().unwrap().push(state.clone());
        LOCAL.with(|l| l.set(Arc::as_ptr(&state)));
        *g = Some(Registered(state));
        true
    })
}

/// Called from the signal handler on the sampled thread with the interrupted registers.
fn on_sample(pc: usize, fp: usize, sp: usize) {
    // SAFETY: LOCAL points at the state this thread's GUARD keeps alive, and is cleared before the guard drops.
    let Some(state) = (unsafe { LOCAL.with(Cell::get).as_ref() }) else {
        return;
    };
    let Some(ring) = state.ring.get() else { return };
    if !state.armed.load(Acquire) {
        return;
    }
    if state.busy.load(Relaxed) {
        ring.dropped.fetch_add(1, Relaxed);
        return;
    }
    let timed = state.seen.fetch_add(1, Relaxed) % TIME_EVERY == 0;
    let started = if timed { os::thread_cpu_ns() } else { 0 };
    if let Some(slot) = ring.reserve() {
        // SAFETY: reserve gave a slot the consumer will not read before publish; a zeroed Record is valid.
        let record = unsafe { &mut *slot };
        // SAFETY: bounds are this thread's stack and pc/fp/sp are its interrupted registers.
        let depth = unsafe { unwind::walk(pc, fp, sp, state.bounds, &mut record.frames) };
        (record.kind, record.depth, record.ctx, record.value) =
            (Kind::Cpu, depth as u8, context(), 0);
        ring.publish();
        state.samples.fetch_add(1, Relaxed);
    }
    if timed {
        state
            .timed_ns
            .fetch_add(os::thread_cpu_ns().saturating_sub(started), Relaxed);
        state.timed.fetch_add(1, Relaxed);
    }
}

/// Ends the current tick or frame (`context().tick`) with its duration, so a report can keep only slow ones.
pub fn tick_end(duration_ns: u64) {
    // SAFETY: LOCAL points at the state this thread's GUARD keeps alive, and is cleared before the guard drops.
    let Some(state) = (unsafe { LOCAL.with(Cell::get).as_ref() }) else {
        return;
    };
    let Some(ring) = state.ring.get().filter(|_| state.armed.load(Acquire)) else {
        return;
    };
    // The handler drops its sample, rather than interleave with ours, while busy is set.
    state.busy.store(true, Relaxed);
    compiler_fence(SeqCst);
    if let Some(slot) = ring.reserve() {
        // SAFETY: reserve gave a slot the consumer will not read before publish; a zeroed Record is valid.
        let record = unsafe { &mut *slot };
        (record.kind, record.depth, record.ctx, record.value) =
            (Kind::Tick, 0, context(), duration_ns);
        ring.publish();
    }
    compiler_fence(SeqCst);
    state.busy.store(false, Relaxed);
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub hz: u32,
    /// Slots of the ring a thread gets at its first sample run (it keeps that ring after).
    pub ring_slots: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hz: DEFAULT_HZ,
            ring_slots: 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub samples: u64,
    pub dropped: u64,
    /// Mean CPU time the handler took per sample, over the 1 in 64 it timed; excludes the kernel's signal delivery.
    pub handler_ns: Option<f64>,
}

/// Threads being sampled; dropping it (or `stop`) cancels their timers.
pub struct Session {
    threads: Vec<Arc<ThreadState>>,
}

/// Starts sampling every registered thread `select` accepts and that no other session samples.
pub fn start(config: Config, select: impl Fn(&str) -> bool) -> io::Result<Session> {
    os::install_handler()?;
    let registry = REGISTRY.lock().unwrap();
    let mut session = Session {
        threads: Vec::new(),
    };
    for state in registry
        .iter()
        .filter(|s| select(s.name) && !s.armed.load(Acquire))
    {
        let ring = match state.ring.get() {
            Some(ring) => ring,
            None => {
                let _ = state.ring.set(Ring::new(config.ring_slots)?);
                state.ring.get().unwrap()
            }
        };
        ring.clear();
        state.seen.store(0, Relaxed);
        for counter in [&state.samples, &state.timed, &state.timed_ns] {
            counter.store(0, Relaxed);
        }
        state.armed.store(true, Release);
        session.threads.push(state.clone());
        *state.timer.lock().unwrap() = Some(os::arm(&state.target, config.hz)?);
    }
    if session.threads.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no sampled thread matches",
        ));
    }
    Ok(session)
}

impl Session {
    /// Hands every record written so far, thread by thread, to `f`.
    pub fn drain(&mut self, mut f: impl FnMut(&str, &Record)) {
        for state in &self.threads {
            state.ring.get().unwrap().drain(|r| f(state.name, r));
        }
    }

    pub fn stats(&self) -> Stats {
        let sum =
            |field: fn(&ThreadState) -> u64| self.threads.iter().map(|s| field(s)).sum::<u64>();
        let timed = sum(|s| s.timed.load(Relaxed));
        Stats {
            samples: sum(|s| s.samples.load(Relaxed)),
            dropped: sum(|s| s.ring.get().map_or(0, |r| r.dropped.load(Relaxed))),
            handler_ns: (timed > 0)
                .then(|| sum(|s| s.timed_ns.load(Relaxed)) as f64 / timed as f64),
        }
    }

    /// Cancels the timers, then drains what is left.
    pub fn stop(mut self, f: impl FnMut(&str, &Record)) -> Stats {
        self.threads.iter().for_each(|s| s.disarm());
        self.drain(f);
        self.stats()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.threads.iter().for_each(|s| s.disarm());
    }
}

/// The CPU samples of ticks that lasted at least `over_ns` (one thread's records, in order).
pub fn slow_tick_samples(records: &[Record], over_ns: u64) -> impl Iterator<Item = &Record> {
    let slow: HashSet<u64> = records
        .iter()
        .filter(|r| r.kind == Kind::Tick && r.value >= over_ns)
        .map(|r| r.ctx.tick)
        .collect();
    records
        .iter()
        .filter(move |r| r.kind == Kind::Cpu && slow.contains(&r.ctx.tick))
}

#[cfg(test)]
mod tests;
