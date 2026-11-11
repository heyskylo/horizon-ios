//! Sampled heap profiling: mimalloc that records the call stack of about one allocation per `interval` bytes while a session runs.
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering::*};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::profile::{Aggregate, Key, State};
use crate::sampler::{self, Regs, StackUnwinder};
use crate::unwind::{self, MAX_FRAMES, StackBounds};

/// Bytes between samples on average when `HORIZON_HEAP_SAMPLE_KIB` is unset.
pub const DEFAULT_INTERVAL: usize = 512 * 1024;
const BUCKET_BITS: u32 = 20;
const SATURATED: u8 = u8::MAX;
const TIME_EVERY: u64 = 16;

static ON: AtomicBool = AtomicBool::new(false);
static OWNED: AtomicBool = AtomicBool::new(false);
static INTERVAL: AtomicUsize = AtomicUsize::new(0);
static SESSION: AtomicU32 = AtomicU32::new(0);
static LIVE_SAMPLES: AtomicUsize = AtomicUsize::new(0);
static BUCKETS: [AtomicU8; 1 << BUCKET_BITS] = [const { AtomicU8::new(0) }; 1 << BUCKET_BITS];
static HEAP: Mutex<Option<Heap>> = Mutex::new(None);
static UNWINDER: Mutex<Option<StackUnwinder>> = Mutex::new(None);
static FRAME_POINTERS: OnceLock<bool> = OnceLock::new();
static SEED: AtomicUsize = AtomicUsize::new(0x9E37_79B9);

thread_local! {
    static LEFT: Cell<isize> = const { Cell::new(0) };
    static EPOCH: Cell<u32> = const { Cell::new(0) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static RNG: Cell<u64> = const { Cell::new(0) };
    static THREAD: Cell<(u32, u16)> = const { Cell::new((0, 0)) };
    static BOUNDS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static SAMPLED: Cell<u64> = const { Cell::new(0) };
    static IGNORED: Cell<bool> = const { Cell::new(false) };
}

/// Never samples the calling thread's allocations: the profiler's own threads call it so their buffers stay out of the profile.
pub fn ignore_this_thread() {
    IGNORED.set(true);
}

/// mimalloc, sampling allocations while a [`Session`] runs.
pub struct SamplingAllocator;

#[inline(always)]
fn bucket(ptr: usize) -> &'static AtomicU8 {
    let h = ((ptr as u64) >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - BUCKET_BITS);
    &BUCKETS[h as usize]
}

#[inline(always)]
fn on_alloc(ptr: *mut u8, size: usize) {
    let left = LEFT.get().wrapping_sub(size as isize);
    if left > 0 && EPOCH.get() == SESSION.load(Relaxed) {
        LEFT.set(left);
    } else {
        crossed(ptr as usize, size);
    }
}

#[inline(always)]
fn on_free(ptr: *mut u8) {
    if LIVE_SAMPLES.load(Relaxed) != 0 && bucket(ptr as usize).load(Relaxed) != 0 {
        freed(ptr as usize);
    }
}

// SAFETY: forwards every call to mimalloc unchanged; sampling only reads the returned pointer and its size.
unsafe impl GlobalAlloc for SamplingAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !ON.load(Relaxed) {
            // SAFETY: the caller's contract.
            return unsafe { mimalloc::MiMalloc.alloc(layout) };
        }
        // SAFETY: the caller's contract.
        sampled(unsafe { mimalloc::MiMalloc.alloc(layout) }, layout.size())
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !ON.load(Relaxed) {
            // SAFETY: the caller's contract.
            return unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) };
        }
        // SAFETY: the caller's contract.
        sampled(
            unsafe { mimalloc::MiMalloc.alloc_zeroed(layout) },
            layout.size(),
        )
    }
    #[inline]
    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        on_free(block);
        // SAFETY: the caller's contract.
        unsafe { mimalloc::MiMalloc.dealloc(block, layout) }
    }
    #[inline]
    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        on_free(block);
        if !ON.load(Relaxed) {
            // SAFETY: the caller's contract.
            return unsafe { mimalloc::MiMalloc.realloc(block, layout, new_size) };
        }
        // SAFETY: the caller's contract.
        sampled(
            unsafe { mimalloc::MiMalloc.realloc(block, layout, new_size) },
            new_size,
        )
    }
}

#[inline(always)]
fn sampled(block: *mut u8, size: usize) -> *mut u8 {
    if !block.is_null() {
        on_alloc(block, size);
    }
    block
}

/// Bytes until the next sample: exponential with mean `interval`.
fn draw(interval: usize) -> isize {
    let mut x = RNG.get();
    if x == 0 {
        x = (&RNG as *const _ as u64) ^ (SEED.fetch_add(0x9E37_79B9, Relaxed) as u64) << 17 | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG.set(x);
    let u = ((x >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    (-u.ln() * interval as f64).clamp(1.0, isize::MAX as f64 / 4.0) as isize
}

/// The bytes one sample of a `size`-byte block stands for (unbiased: size over its chance of being sampled).
pub fn weight(size: usize, interval: usize) -> f64 {
    let (s, t) = (size.max(1) as f64, interval.max(1) as f64);
    s / -(-s / t).exp_m1()
}

#[cold]
#[inline(never)]
fn crossed(ptr: usize, size: usize) {
    if BUSY.get() {
        return;
    }
    let interval = INTERVAL.load(Relaxed);
    if interval == 0 {
        return;
    }
    let session = SESSION.load(Relaxed);
    if IGNORED.get() {
        EPOCH.set(session);
        LEFT.set(isize::MAX / 2);
        return;
    }
    if EPOCH.get() != session {
        EPOCH.set(session);
        let left = draw(interval).saturating_sub(size as isize);
        if left > 0 {
            LEFT.set(left);
            return;
        }
    }
    LEFT.set(draw(interval));
    BUSY.set(true);
    record(ptr, size, interval, session);
    BUSY.set(false);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn record(ptr: usize, size: usize, interval: usize, session: u32) {
    let n = SAMPLED.get();
    SAMPLED.set(n + 1);
    let timed = n.is_multiple_of(TIME_EVERY);
    let now = Instant::now();
    let mut frames = [0usize; MAX_FRAMES];
    let depth = capture(&mut frames);
    let ctx = sampler::context();
    let weight = weight(size, interval).round() as u64;
    let mut guard = lock(&HEAP);
    let Some(heap) = guard.as_mut().filter(|h| h.session == session) else {
        return;
    };
    let thread = match THREAD.get() {
        (s, i) if s == session => i,
        _ => {
            let mut buf = [0u8; 64];
            let name = match sampler::sampled_name() {
                Some(name) => name,
                None => match std::str::from_utf8(crate::heap_census::thread_name(&mut buf)) {
                    Ok("") | Err(_) => "unnamed thread",
                    Ok(name) => name,
                },
            };
            let i = heap.agg.thread_index(name);
            THREAD.set((session, i));
            i
        }
    };
    let elapsed = now.saturating_duration_since(heap.started).as_secs_f64();
    heap.agg.set_time_block((elapsed / heap.block_s) as usize);
    let key = heap
        .agg
        .stack_key(thread, State::Alloc, &ctx, &frames[..depth]);
    heap.agg.add_weight(key, weight);
    let live = Sampled {
        key: Key {
            state: State::Live,
            ..key
        },
        weight,
    };
    if heap.live.insert(ptr, live).is_none() {
        let b = bucket(ptr);
        let c = b.load(Relaxed);
        if c != SATURATED {
            b.store(c + 1, Relaxed);
        }
        LIVE_SAMPLES.fetch_add(1, Relaxed);
    }
    heap.samples += 1;
    if timed {
        heap.timed += 1;
        heap.timed_ns += now.elapsed().as_nanos() as u64;
    }
}

#[cold]
#[inline(never)]
fn freed(ptr: usize) {
    if BUSY.replace(true) {
        return;
    }
    let mut guard = lock(&HEAP);
    if let Some(heap) = guard.as_mut()
        && let Some(s) = heap.live.remove(&ptr)
    {
        heap.freed += s.weight;
        unbucket(ptr);
        LIVE_SAMPLES.fetch_sub(1, Relaxed);
    }
    drop(guard);
    BUSY.set(false);
}

fn unbucket(ptr: usize) {
    let b = bucket(ptr);
    let c = b.load(Relaxed);
    if c != SATURATED && c != 0 {
        b.store(c - 1, Relaxed);
    }
}

fn bounds() -> Option<StackBounds> {
    let (lo, hi) = match BOUNDS.get() {
        (0, 0) => {
            let b = StackBounds::current().map_or((1, 1), |b| (b.lo, b.hi));
            BOUNDS.set(b);
            b
        }
        b => b,
    };
    (lo < hi).then_some(StackBounds { lo, hi })
}

/// The calling thread's stack, innermost first: frame pointers when the build keeps them, else its `.eh_frame` tables.
#[inline(never)]
fn capture(out: &mut [usize]) -> usize {
    let Some(bounds) = bounds() else {
        return 0;
    };
    if *FRAME_POINTERS.get().unwrap_or(&true) {
        let here = 0u8;
        // SAFETY: the calling thread's own live stack; the address of a local is at or below every frame read.
        return unsafe {
            unwind::walk(
                capture as *const () as usize,
                unwind::frame_pointer(),
                std::hint::black_box(&here) as *const u8 as usize,
                bounds,
                out,
            )
        };
    }
    let mut unwinder = lock(&UNWINDER);
    let Some(unwinder) = unwinder.as_mut() else {
        return 0;
    };
    let regs = registers();
    if regs.sp < bounds.lo || regs.sp >= bounds.hi {
        return 0;
    }
    let read = |addr: u64| -> Result<u64, ()> {
        let addr = usize::try_from(addr).map_err(|_| ())?;
        if addr < regs.sp || addr > bounds.hi - 8 || addr % 8 != 0 {
            return Err(());
        }
        // SAFETY: an aligned word of this thread's mapped stack at or above the sp of a frame that is still live.
        Ok(unsafe { std::ptr::read_volatile(addr as *const u64) })
    };
    unwinder.unwind_with(&regs, read, out)
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn registers() -> Regs {
    let (pc, sp, fp): (usize, usize, usize);
    // SAFETY: reads three registers; no memory or flags touched.
    unsafe {
        std::arch::asm!("lea {}, [rip]", "mov {}, rsp", "mov {}, rbp", out(reg) pc, out(reg) sp, out(reg) fp, options(nomem, nostack, preserves_flags))
    };
    Regs { pc, fp, sp, lr: 0 }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn registers() -> Regs {
    let (pc, sp, fp, lr): (usize, usize, usize, usize);
    // SAFETY: reads four registers; no memory or flags touched.
    unsafe {
        std::arch::asm!("adr {}, .", "mov {}, sp", "mov {}, x29", "mov {}, x30", out(reg) pc, out(reg) sp, out(reg) fp, out(reg) lr, options(nomem, nostack, preserves_flags))
    };
    Regs { pc, fp, sp, lr }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline(always)]
fn registers() -> Regs {
    Regs::default()
}

#[derive(Default)]
struct AddrHash(u64);

impl Hasher for AddrHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

#[derive(Clone, Copy)]
struct Sampled {
    key: Key,
    weight: u64,
}

struct Heap {
    session: u32,
    agg: Aggregate,
    live: HashMap<usize, Sampled, BuildHasherDefault<AddrHash>>,
    started: Instant,
    block_s: f64,
    interval: usize,
    freed: u64,
    samples: u64,
    timed: u64,
    timed_ns: u64,
}

impl Heap {
    fn profile(&self, mut agg: Aggregate) -> HeapProfile {
        for s in self.live.values() {
            agg.add_weight(s.key, s.weight);
        }
        HeapProfile {
            agg,
            interval: self.interval,
            samples: self.samples,
            live_samples: self.live.len(),
            sample_ns: if self.timed > 0 {
                (self.timed_ns as f64 / self.timed as f64 * self.samples as f64) as u64
            } else {
                0
            },
            freed: self.freed,
            elapsed: self.started.elapsed(),
        }
    }
}

/// What a heap session recorded: `agg` counts bytes under `State::Alloc` (every sample) and `State::Live` (still allocated).
pub struct HeapProfile {
    pub agg: Aggregate,
    pub interval: usize,
    pub samples: u64,
    pub live_samples: usize,
    /// Time spent recording samples, estimated from one in 16.
    pub sample_ns: u64,
    /// Sampled bytes freed during the session.
    pub freed: u64,
    pub elapsed: Duration,
}

impl HeapProfile {
    pub fn bytes(&self, state: State) -> u64 {
        self.agg.total(state, false)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Mean bytes between samples; 0 turns sampling off.
    pub interval: usize,
    /// Seconds per time block, as the CPU profile's.
    pub block_s: f32,
}

impl Config {
    /// The interval from `HORIZON_HEAP_SAMPLE_KIB`, else [`DEFAULT_INTERVAL`].
    pub fn from_env(block_s: f32) -> Self {
        let interval = std::env::var("HORIZON_HEAP_SAMPLE_KIB")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .map_or(DEFAULT_INTERVAL, |kib| kib.saturating_mul(1024));
        Self { interval, block_s }
    }
}

/// The process's one heap session; dropping it stops sampling.
pub struct Session {
    session: u32,
    stopped: bool,
}

/// Starts sampling every thread's allocations; None when the interval is 0 or a session already runs.
pub fn start(config: Config) -> Option<Session> {
    if config.interval == 0 || OWNED.swap(true, AcqRel) {
        return None;
    }
    let was = BUSY.replace(true);
    let fp = *FRAME_POINTERS.get_or_init(unwind::frame_pointers_present);
    *lock(&UNWINDER) = (!fp).then(|| {
        let mut u = StackUnwinder::new();
        u.refresh();
        u
    });
    let session = SESSION.fetch_add(1, Relaxed).wrapping_add(1);
    *lock(&HEAP) = Some(Heap {
        session,
        agg: Aggregate::new(0),
        live: HashMap::default(),
        started: Instant::now(),
        block_s: f64::from(config.block_s.max(0.001)),
        interval: config.interval,
        freed: 0,
        samples: 0,
        timed: 0,
        timed_ns: 0,
    });
    INTERVAL.store(config.interval, Relaxed);
    ON.store(true, Release);
    BUSY.set(was);
    Some(Session {
        session,
        stopped: false,
    })
}

impl Session {
    /// The profile so far, sampling going on.
    pub fn snapshot(&self) -> Option<HeapProfile> {
        let was = BUSY.replace(true);
        let guard = lock(&HEAP);
        let profile = guard
            .as_ref()
            .filter(|h| h.session == self.session)
            .map(|h| h.profile(h.agg.clone()));
        drop(guard);
        BUSY.set(was);
        profile
    }

    pub fn stop(mut self) -> Option<HeapProfile> {
        self.finish()
    }

    fn finish(&mut self) -> Option<HeapProfile> {
        if std::mem::replace(&mut self.stopped, true) {
            return None;
        }
        ON.store(false, Relaxed);
        let was = BUSY.replace(true);
        let mut guard = lock(&HEAP);
        let heap = guard.take().filter(|h| h.session == self.session);
        if let Some(heap) = &heap {
            for &ptr in heap.live.keys() {
                unbucket(ptr);
            }
            LIVE_SAMPLES.store(0, Relaxed);
        }
        drop(guard);
        *lock(&UNWINDER) = None;
        let profile = heap.map(|mut h| {
            let agg = std::mem::replace(&mut h.agg, Aggregate::new(0));
            h.profile(agg)
        });
        OWNED.store(false, Release);
        BUSY.set(was);
        profile
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests;
