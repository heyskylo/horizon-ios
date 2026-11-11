//! In-process stack sampling of named threads: per-thread rings filled by a signal handler, nothing running while off.

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize,
};
use std::sync::atomic::{Ordering::*, compiler_fence};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::unwind::{self, MAX_FRAMES, StackBounds};

mod eh;
mod linux;

pub(crate) use eh::{Loaded, StackUnwinder, loaded_modules};
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
    /// A wall-clock sample: `value` is 1 when the thread's CPU clock advanced since its previous wall sample, else 0.
    Wall = 2,
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

/// The name the calling thread opted in to sampling under, if it did.
pub(crate) fn sampled_name() -> Option<&'static str> {
    // SAFETY: LOCAL points at the state this thread's GUARD keeps alive, and is cleared before the guard drops.
    unsafe { LOCAL.with(Cell::get).as_ref() }.map(|s| s.name)
}

pub fn span() -> u16 {
    CTX.with(|c| c.span.load(Relaxed))
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

/// The interrupted registers; `lr` is the link register on aarch64 and 0 elsewhere.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Regs {
    pub(crate) pc: usize,
    pub(crate) fp: usize,
    pub(crate) sp: usize,
    #[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
    pub(crate) lr: usize,
}

const TAG_PAD: u8 = 0;
const TAG_FRAMES: u8 = 1;
const TAG_COPY: u8 = 2;
const TAG_TICK: u8 = 3;
/// Set in a stack entry's `value` for a wall sample; bit 0 is then "was running".
const WALL: u64 = 2;

/// Starts every ring entry; a pad entry needs only `len` and `tag`. The payload follows: frames or a stack copy.
#[repr(C)]
struct Header {
    len: u32,
    tag: u8,
    payload: u32,
    ctx: Context,
    value: u64,
    regs: Regs,
}

const HDR: usize = std::mem::size_of::<Header>();

const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

struct Slot {
    header: *mut Header,
    total: usize,
    advance: usize,
}

impl Slot {
    fn payload(&self) -> *mut u8 {
        // SAFETY: reserve left `total - HDR` bytes after the header inside the ring.
        unsafe { self.header.cast::<u8>().add(HDR) }
    }

    fn write(&self, tag: u8, payload: usize, value: u64, regs: Regs) {
        let header = Header {
            len: self.total as u32,
            tag,
            payload: payload as u32,
            ctx: context(),
            value,
            regs,
        };
        // SAFETY: reserve gave this slot exclusively and 8-aligned until publish.
        unsafe { self.header.write(header) };
    }
}

/// Single-producer, single-consumer byte ring of variable-length entries; the producer is one thread (handler or marker, never both at once).
struct Ring {
    buf: *mut u8,
    cap: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

// SAFETY: entries are written only by the owning thread before `head` is published and read only below `head`.
unsafe impl Send for Ring {}
// SAFETY: see Send; the indices are atomics.
unsafe impl Sync for Ring {}

impl Ring {
    fn new(bytes: usize) -> io::Result<Self> {
        let cap = align8(bytes.max(2 * HDR));
        let layout = Layout::from_size_align(cap, 8).map_err(io::Error::other)?;
        // SAFETY: the layout is non-zero sized.
        let buf = unsafe { alloc_zeroed(layout) };
        if buf.is_null() {
            return Err(io::ErrorKind::OutOfMemory.into());
        }
        Ok(Self {
            buf,
            cap,
            head: 0.into(),
            tail: 0.into(),
            dropped: 0.into(),
        })
    }

    fn reserve(&self, payload: usize) -> Option<Slot> {
        let total = align8(HDR + payload);
        let (head, tail) = (self.head.load(Relaxed), self.tail.load(Acquire));
        let off = head % self.cap;
        let pad = if off + total > self.cap {
            self.cap - off
        } else {
            0
        };
        if pad + total > self.cap - head.wrapping_sub(tail) {
            self.dropped.fetch_add(1, Relaxed);
            return None;
        }
        // SAFETY: off + pad + total lies within the allocation, and the consumer reads none of it before publish.
        unsafe {
            if pad > 0 {
                let at = self.buf.add(off);
                (at as *mut u32).write(pad as u32);
                at.add(4).write(TAG_PAD);
            }
            Some(Slot {
                header: self.buf.add(if pad > 0 { 0 } else { off }).cast(),
                total,
                advance: pad + total,
            })
        }
    }

    fn publish(&self, slot: &Slot) {
        let head = self.head.load(Relaxed);
        self.head.store(head.wrapping_add(slot.advance), Release);
    }

    fn clear(&self) {
        self.tail.store(self.head.load(Acquire), Release);
        self.dropped.store(0, Relaxed);
    }

    fn drain(&self, mut f: impl FnMut(&Header, &[u8])) {
        let (mut at, head) = (self.tail.load(Relaxed), self.head.load(Acquire));
        while at != head {
            // SAFETY: entries below the acquired head were fully written before the producer published them; len and tag sit in every entry, even an 8-byte pad.
            let (entry, len, tag) = unsafe {
                let entry = self.buf.add(at % self.cap);
                (entry, entry.cast::<u32>().read(), entry.add(4).read())
            };
            if tag != TAG_PAD {
                // SAFETY: a non-pad entry holds a whole header, then `payload` bytes.
                let (header, payload) = unsafe {
                    let header = &*entry.cast::<Header>();
                    (
                        header,
                        std::slice::from_raw_parts(entry.add(HDR), header.payload as usize),
                    )
                };
                f(header, payload);
            }
            at = at.wrapping_add(len as usize);
        }
        self.tail.store(head, Release);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        // SAFETY: the same layout `new` allocated with.
        unsafe { dealloc(self.buf, Layout::from_size_align(self.cap, 8).unwrap()) }
    }
}

#[derive(Default)]
struct ThreadState {
    name: &'static str,
    target: os::Target,
    bounds: StackBounds,
    /// The ring samples go to: the last of `rings`, null before the first session.
    ring: AtomicPtr<Ring>,
    /// Every ring this thread has had; a replaced one stays allocated, as a late signal's handler may still hold it.
    #[expect(clippy::vec_box, reason = "`ring` points at a Ring, which must not move")]
    rings: Mutex<Vec<Box<Ring>>>,
    armed: AtomicBool,
    /// Bytes of stack each sample copies; 0 means the handler walks frame pointers instead.
    copy_bytes: AtomicUsize,
    busy: AtomicBool,
    seen: AtomicU32,
    samples: AtomicU64,
    timed: AtomicU64,
    timed_ns: AtomicU64,
    timer: Mutex<Option<os::Timer>>,
    wall_timer: Mutex<Option<os::Timer>>,
    wall_period_ns: AtomicU64,
    wall_cpu_ns: AtomicU64,
}

impl ThreadState {
    fn ring(&self) -> Option<&Ring> {
        // SAFETY: a non-null pointer is a box in `rings`, which only grows while self lives.
        unsafe { self.ring.load(Acquire).as_ref() }
    }

    /// The ring, made anew when the current one is smaller than `bytes`.
    fn ring_of(&self, bytes: usize) -> io::Result<&Ring> {
        if let Some(ring) = self.ring().filter(|r| r.cap >= align8(bytes.max(2 * HDR))) {
            return Ok(ring);
        }
        let ring = Box::new(Ring::new(bytes)?);
        let at = std::ptr::from_ref(&*ring).cast_mut();
        self.rings.lock().unwrap().push(ring);
        self.ring.store(at, Release);
        Ok(self.ring().unwrap())
    }

    fn disarm(&self) {
        self.armed.store(false, Release);
        for slot in [&self.timer, &self.wall_timer] {
            if let Some(timer) = slot.lock().unwrap().take() {
                os::disarm(timer);
            }
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

/// Called from the signal handler on the sampled thread with the interrupted registers; `wall` is the wall-clock timer's signal.
fn on_sample(regs: Regs, wall: bool) {
    // SAFETY: LOCAL points at the state this thread's GUARD keeps alive, and is cleared before the guard drops.
    let Some(state) = (unsafe { LOCAL.with(Cell::get).as_ref() }) else {
        return;
    };
    let Some(ring) = state.ring() else { return };
    if !state.armed.load(Acquire) {
        return;
    }
    if state.busy.load(Relaxed) {
        ring.dropped.fetch_add(1, Relaxed);
        return;
    }
    let timed = state.seen.fetch_add(1, Relaxed) % TIME_EVERY == 0;
    let started = if timed { os::thread_cpu_ns() } else { 0 };
    // Running means most of the interval went to CPU time; the handlers' own few microseconds don't count.
    let tag = if wall {
        let now = os::thread_cpu_ns();
        let used = now.saturating_sub(state.wall_cpu_ns.swap(now, Relaxed));
        WALL | u64::from(used > state.wall_period_ns.load(Relaxed) / 2)
    } else {
        0
    };
    let (copy, bounds) = (state.copy_bytes.load(Relaxed), state.bounds);
    let on_stack = (bounds.lo..bounds.hi).contains(&regs.sp);
    let copied = if on_stack {
        copy.min(bounds.hi - regs.sp)
    } else {
        0
    };
    if let Some(slot) = ring.reserve(if copy == 0 { MAX_FRAMES * 8 } else { copied }) {
        if copy == 0 {
            // SAFETY: the slot's payload holds MAX_FRAMES words, 8-aligned.
            let frames =
                unsafe { std::slice::from_raw_parts_mut(slot.payload().cast(), MAX_FRAMES) };
            // SAFETY: bounds are this thread's stack and the registers are its interrupted ones.
            let depth = unsafe { unwind::walk(regs.pc, regs.fp, regs.sp, bounds, frames) };
            slot.write(TAG_FRAMES, depth * 8, tag, regs);
        } else {
            // SAFETY: [sp, sp + copied) lies inside this thread's mapped stack and the slot's payload holds that many bytes.
            unsafe { std::ptr::copy_nonoverlapping(regs.sp as *const u8, slot.payload(), copied) };
            slot.write(TAG_COPY, copied, tag, regs);
        }
        ring.publish(&slot);
        state.samples.fetch_add(1, Relaxed);
    }
    if timed {
        state
            .timed_ns
            .fetch_add(os::thread_cpu_ns().saturating_sub(started), Relaxed);
        state.timed.fetch_add(1, Relaxed);
    }
}

/// The calling thread's CPU clock in nanoseconds.
pub fn thread_cpu_ns() -> u64 {
    os::thread_cpu_ns()
}

/// Ends the current tick or frame (`context().tick`) with its duration, so a report can keep only slow ones.
pub fn tick_end(duration_ns: u64) {
    // SAFETY: LOCAL points at the state this thread's GUARD keeps alive, and is cleared before the guard drops.
    let Some(state) = (unsafe { LOCAL.with(Cell::get).as_ref() }) else {
        return;
    };
    let Some(ring) = state.ring().filter(|_| state.armed.load(Acquire)) else {
        return;
    };
    // The handler drops its sample, rather than interleave with ours, while busy is set.
    state.busy.store(true, Relaxed);
    compiler_fence(SeqCst);
    if let Some(slot) = ring.reserve(0) {
        slot.write(TAG_TICK, 0, duration_ns, Regs::default());
        ring.publish(&slot);
    }
    compiler_fence(SeqCst);
    state.busy.store(false, Relaxed);
}

/// How a sample's stack is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unwind {
    /// Frame pointers when this build keeps them (dev builds), else a stack copy.
    Auto,
    /// The handler walks frame pointers; needs `-C force-frame-pointers=yes` throughout.
    FramePointer,
    /// The handler copies the top of the stack and the draining thread unwinds it with `.eh_frame`.
    StackCopy,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub hz: u32,
    /// Samples a thread's ring holds; 0 is two seconds' worth. A session that needs more than the thread's ring holds gets a larger one.
    pub ring_samples: usize,
    pub unwind: Unwind,
    /// Stack bytes copied per sample in stack-copy mode, in KiB (clamped to the thread's stack).
    pub stack_copy_kib: usize,
    /// Also arm a wall-clock timer per thread at the same rate, to find waits (`Kind::Wall`).
    pub wall: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hz: DEFAULT_HZ,
            ring_samples: 0,
            unwind: Unwind::Auto,
            stack_copy_kib: 16,
            wall: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub samples: u64,
    pub dropped: u64,
    /// Mean CPU time the handler took per sample, over the 1 in 64 it timed; excludes the kernel's signal delivery.
    pub handler_ns: Option<f64>,
    /// Mean wall time the draining thread spent unwinding one stack copy, module loading excluded.
    pub unwind_ns: Option<f64>,
}

/// Threads being sampled; dropping it (or `stop`) cancels their timers.
pub struct Session {
    threads: Vec<Arc<ThreadState>>,
    unwinder: Option<eh::StackUnwinder>,
    unwound: u64,
    unwind_ns: u64,
}

/// Starts sampling every registered thread `select` accepts and that no other session samples.
pub fn start(config: Config, select: impl Fn(&str) -> bool) -> io::Result<Session> {
    os::install_handler()?;
    let copy = match config.unwind {
        Unwind::FramePointer => 0,
        Unwind::Auto if unwind::frame_pointers_present() => 0,
        _ => config.stack_copy_kib.max(1) * 1024,
    };
    let samples = match config.ring_samples {
        0 => 2 * config.hz as usize,
        n => n,
    } * (1 + usize::from(config.wall));
    let ring_bytes = samples.max(2) * align8(HDR + copy.max(MAX_FRAMES * 8));
    let registry = REGISTRY.lock().unwrap();
    let mut session = Session {
        threads: Vec::new(),
        unwinder: None,
        unwound: 0,
        unwind_ns: 0,
    };
    for state in registry
        .iter()
        .filter(|s| select(s.name) && !s.armed.load(Acquire))
    {
        state.ring_of(ring_bytes)?.clear();
        state.seen.store(0, Relaxed);
        for counter in [&state.samples, &state.timed, &state.timed_ns] {
            counter.store(0, Relaxed);
        }
        state.copy_bytes.store(copy, Relaxed);
        state.armed.store(true, Release);
        session.threads.push(state.clone());
        *state.timer.lock().unwrap() = Some(os::arm(&state.target, config.hz)?);
        if config.wall {
            let period = 1_000_000_000 / u64::from(config.hz.max(1));
            state.wall_period_ns.store(period, Relaxed);
            state.wall_cpu_ns.store(0, Relaxed);
            *state.wall_timer.lock().unwrap() = Some(os::arm_wall(&state.target, config.hz)?);
        }
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
    /// Hands every record written so far, thread by thread, to `f`; stack copies are unwound here, on the calling thread.
    pub fn drain(&mut self, mut f: impl FnMut(&str, &Record)) {
        let mut refreshed = false;
        for state in &self.threads {
            state.ring().unwrap().drain(|header, payload| {
                let mut record = Record {
                    kind: Kind::Cpu,
                    depth: 0,
                    ctx: header.ctx,
                    value: header.value,
                    frames: [0; MAX_FRAMES],
                };
                if header.tag != TAG_TICK && header.value & WALL != 0 {
                    record.kind = Kind::Wall;
                    record.value = header.value & 1;
                }
                let depth = match header.tag {
                    TAG_TICK => {
                        record.kind = Kind::Tick;
                        0
                    }
                    TAG_FRAMES => {
                        let (words, _) = payload.as_chunks::<8>();
                        for (slot, w) in record.frames.iter_mut().zip(words) {
                            *slot = usize::from_ne_bytes(*w);
                        }
                        payload.len() / 8
                    }
                    _ => {
                        let unwinder = self.unwinder.get_or_insert_with(eh::StackUnwinder::new);
                        if !std::mem::replace(&mut refreshed, true) {
                            unwinder.refresh();
                        }
                        let started = Instant::now();
                        let depth = unwinder.unwind(&header.regs, payload, &mut record.frames);
                        self.unwind_ns += started.elapsed().as_nanos() as u64;
                        self.unwound += 1;
                        depth
                    }
                };
                record.depth = depth as u8;
                f(state.name, &record);
            });
        }
    }

    pub fn stats(&self) -> Stats {
        let sum =
            |field: fn(&ThreadState) -> u64| self.threads.iter().map(|s| field(s)).sum::<u64>();
        let timed = sum(|s| s.timed.load(Relaxed));
        Stats {
            samples: sum(|s| s.samples.load(Relaxed)),
            dropped: sum(|s| s.ring().map_or(0, |r| r.dropped.load(Relaxed))),
            handler_ns: (timed > 0)
                .then(|| sum(|s| s.timed_ns.load(Relaxed)) as f64 / timed as f64),
            unwind_ns: (self.unwound > 0).then(|| self.unwind_ns as f64 / self.unwound as f64),
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
