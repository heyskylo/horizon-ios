//! Named timings (wall and thread CPU) of a frame or tick, on with `HORIZON_SPIKES` or while a capture records; the slow ones go to the slow ring, not as span lists to the log.
use std::{
    cell::{Cell, RefCell},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static ENABLED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static SPANS: RefCell<Vec<Span>> = const { RefCell::new(Vec::new()) };
    /// Spans recorded on this thread alone, wall time only ([`record_here`]).
    static HERE: Cell<bool> = const { Cell::new(false) };
    /// How many timed spans the thread is inside: the next span's [`Span::depth`].
    static DEPTH: Cell<u8> = const { Cell::new(0) };
    /// What this thread's slowest steps were busy with since it last took them ([`note`]).
    static NOTES: RefCell<Vec<(f32, String)>> = const { RefCell::new(Vec::new()) };
}

/// The most spans a thread holds before they are taken ([`take_raw`]): a worker thread
/// that records spans (`lod_reduce`, `lt_*`) while they are on for every thread is never
/// taken from, so it drops what it held instead of growing for as long as they stay on.
const UNTAKEN_MAX: usize = 1 << 14;

/// The shortest step a [`note`] keeps, in milliseconds.
pub const NOTE_MS: f32 = 10.0;
/// The notes a thread keeps between two [`take_notes`]: its slowest steps.
const NOTES_KEPT: usize = 3;

/// A named timing: wall milliseconds and the thread's CPU milliseconds in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub name: &'static str,
    pub ms: f32,
    pub cpu_ms: f32,
    /// How many timed spans it ran inside (0: none): `tick_mobs` is inside `tick`.
    pub depth: u8,
}

pub fn enable(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}
/// Whether spans are being recorded.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
/// Record this thread's spans even with spans off, their wall time only (no CPU clock read):
/// what the render thread's slow frames spent their time in, for the opt-in performance
/// statistics (`telemetry::Session::frame`). The thread takes them every frame ([`take`]).
pub fn record_here(on: bool) {
    HERE.with(|h| h.set(on));
}
/// Whether this thread records spans: all of them on, or [`record_here`].
#[inline]
pub fn recording_here() -> bool {
    ENABLED.load(Ordering::Relaxed) || HERE.with(Cell::get) || timeline::on()
}
/// The thread's CPU time when spans are on for every thread (`HORIZON_SPIKES`): spans recorded
/// by `record_here` alone keep wall time only.
#[inline]
fn cpu_if_enabled() -> Option<Duration> {
    if ENABLED.load(Ordering::Relaxed) {
        thread_cpu()
    } else {
        None
    }
}
/// Run `f`, recording its duration under `name` when spans are enabled.
#[inline]
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let _span = guard(name);
    f()
}
/// A span that ends where it is dropped: [`time`] for a step with several ways out (early
/// returns). Nothing is read while spans are off.
#[must_use = "the span ends when the guard is dropped"]
pub struct Guard {
    open: Option<(&'static str, Instant, Option<Duration>, u8)>,
    _heap: Option<horizon_threads::heap_census::Tag>,
    /// The sampler's span before this one.
    outer: u16,
}
/// Start a span named `name` that ends when the returned [`Guard`] is dropped.
#[inline]
pub fn guard(name: &'static str) -> Guard {
    let _heap = horizon_threads::heap_tag(name);
    let outer = horizon_threads::work::enter_span(name);
    if !recording_here() {
        return Guard {
            open: None,
            _heap,
            outer,
        };
    }
    let depth = DEPTH.with(|d| d.replace(d.get().saturating_add(1)));
    Guard {
        open: Some((name, Instant::now(), cpu_if_enabled(), depth)),
        _heap,
        outer,
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        horizon_threads::work::leave_span(self.outer);
        let Some((name, started, cpu, depth)) = self.open.take() else {
            return;
        };
        let elapsed = started.elapsed();
        let ms = elapsed.as_secs_f32() * 1000.0;
        let cpu_ms = cpu
            .and_then(|start| Some((thread_cpu()?, start)))
            .map_or(ms, |(end, start)| (end - start).as_secs_f32() * 1000.0);
        DEPTH.with(|d| d.set(depth));
        timeline::span(name, started, elapsed, depth);
        push(Span {
            name,
            ms,
            cpu_ms,
            depth,
        });
    }
}
/// How many spans this thread holds.
fn recorded() -> usize {
    SPANS.with(|s| s.borrow().len())
}
/// Moves the spans recorded since `mark` one level in: they ran inside the lap now ending.
fn nest_since(mark: usize, depth: u8) {
    SPANS.with(|s| {
        let mut s = s.borrow_mut();
        let start = mark.min(s.len());
        for span in &mut s[start..] {
            if span.depth >= depth {
                span.depth = span.depth.saturating_add(1);
            }
        }
    });
}
/// Keeps a finished span for the thread's next [`take_raw`].
fn push(span: Span) {
    SPANS.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() >= UNTAKEN_MAX {
            s.clear();
        }
        s.push(span);
    });
}
/// Notes that one step inside a span took `ms` milliseconds doing `what` (one player's
/// packets, say), when this thread records spans and the step took at least [`NOTE_MS`]: a
/// span says where a slow tick went, its notes whose doing it was. Only the slowest few are
/// kept, and `what` is built only for a step that is.
pub fn note(ms: f32, what: impl FnOnce() -> String) {
    if ms < NOTE_MS || !recording_here() {
        return;
    }
    NOTES.with(|n| {
        let mut n = n.borrow_mut();
        if n.len() >= NOTES_KEPT {
            let Some((i, _)) = n
                .iter()
                .enumerate()
                .filter(|(_, (kept, _))| *kept < ms)
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
            else {
                return;
            };
            n.swap_remove(i);
        }
        n.push((ms, what()));
    });
}
/// This thread's notes ([`note`]), slowest first; clears them.
pub fn take_notes() -> Vec<(f32, String)> {
    let mut notes = NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()));
    notes.sort_by(|a, b| b.0.total_cmp(&a.0));
    notes
}
/// Consecutive steps of one function timed as spans: each [`Laps::lap`] records the time since
/// the previous one (or since [`laps`]) under its name. Nothing is read while spans are off.
pub struct Laps(Option<(Instant, Option<Duration>, usize)>);
/// Start timing the steps of a function (see [`Laps`]).
#[inline]
pub fn laps() -> Laps {
    Laps(recording_here().then(|| (Instant::now(), cpu_if_enabled(), recorded())))
}
impl Laps {
    /// Record the step that just ended under `name`, and start the next.
    #[inline]
    pub fn lap(&mut self, name: &'static str) {
        let Some((started, cpu, mark)) = &mut self.0 else {
            return;
        };
        let now = Instant::now();
        let now_cpu = cpu.and_then(|_| thread_cpu());
        let elapsed = now - *started;
        let ms = elapsed.as_secs_f32() * 1000.0;
        let cpu_ms = now_cpu
            .zip(*cpu)
            .map_or(ms, |(end, start)| (end - start).as_secs_f32() * 1000.0);
        let depth = DEPTH.with(Cell::get);
        timeline::span(name, *started, elapsed, depth);
        nest_since(*mark, depth);
        push(Span {
            name,
            ms,
            cpu_ms,
            depth,
        });
        (*started, *cpu, *mark) = (now, now_cpu, recorded());
    }
}
/// The CPU time this thread has used, where the platform tells it: a tick
/// with more wall time than CPU time waited (for a lock, or for a core on a
/// busy machine).
pub fn thread_cpu() -> Option<Duration> {
    #[cfg(unix)]
    {
        let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        Some(Duration::new(t.tv_sec as u64, t.tv_nsec as u32))
    }
    #[cfg(not(unix))]
    horizon_threads::thread_cpu()
}

/// The frame's spans as they were recorded, each after the spans inside it; clears them.
pub fn take_raw() -> Vec<Span> {
    SPANS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}
/// The frame's spans, summed by name, slowest first; clears them.
pub fn take() -> Vec<Span> {
    merge(&take_raw())
}
/// What a frame spent its time in, from its spans as recorded ([`take_raw`]): the slowest of
/// the outermost spans (summed by name, as a frame can tick several times), then the slowest
/// span inside it while that one took at least half of it, and so on. The path of names
/// (`tick/tick_mobs`) and the last one's milliseconds; `None` without spans.
pub fn culprit(raw: &[Span]) -> Option<(String, f32)> {
    // The spans directly inside each one: recorded just before it, one level deeper.
    let mut inside: Vec<Vec<usize>> = vec![Vec::new(); raw.len()];
    let mut outer: Vec<usize> = Vec::new();
    for (i, span) in raw.iter().enumerate() {
        while let Some(&j) = outer.last()
            && raw[j].depth > span.depth
        {
            outer.pop();
            inside[i].push(j);
        }
        outer.push(i);
    }
    let slowest = |group: &[usize]| {
        let mut sums: Vec<(&'static str, f32, Vec<usize>)> = Vec::new();
        for &i in group {
            match sums.iter_mut().find(|s| s.0 == raw[i].name) {
                Some(sum) => {
                    sum.1 += raw[i].ms;
                    sum.2.push(i);
                }
                None => sums.push((raw[i].name, raw[i].ms, vec![i])),
            }
        }
        sums.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))
    };
    let (name, mut ms, mut at) = slowest(&outer)?;
    let mut path = name.to_owned();
    loop {
        let next: Vec<usize> = at.iter().flat_map(|&i| inside[i].iter().copied()).collect();
        match slowest(&next) {
            Some((name, inner_ms, spans)) if inner_ms >= ms * 0.5 => {
                path.push('/');
                path.push_str(name);
                (ms, at) = (inner_ms, spans);
            }
            _ => return Some((path, ms)),
        }
    }
}
/// Spans summed by name, slowest first.
pub fn merge(spans: &[Span]) -> Vec<Span> {
    let mut sums: Vec<Span> = Vec::new();
    for &span in spans {
        match sums.iter_mut().find(|s| s.name == span.name) {
            Some(sum) => {
                sum.ms += span.ms;
                sum.cpu_ms += span.cpu_ms;
            }
            None => sums.push(span),
        }
    }
    sums.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    sums
}
/// The run's timeline: every span on every thread, kept in per-thread buffers until a capture drains them as `timeline` chunks.
pub mod timeline {
    use std::{
        cell::RefCell,
        sync::{
            Arc, Mutex, PoisonError,
            atomic::{AtomicBool, AtomicU32, Ordering},
        },
        time::{Duration, Instant},
    };

    /// The most spans a thread holds between two drains; a thread that records more drops them.
    const THREAD_EVENTS_MAX: usize = 1 << 18;

    static ON: AtomicBool = AtomicBool::new(false);
    static GENERATION: AtomicU32 = AtomicU32::new(0);
    static RUN: Mutex<Option<Run>> = Mutex::new(None);

    struct Run {
        origin: Instant,
        threads: Vec<Arc<Mutex<Thread>>>,
    }

    struct Thread {
        name: String,
        label: String,
        events: Vec<Event>,
    }

    /// This thread's buffer in the timeline it last joined.
    struct Mine {
        generation: u32,
        origin: Instant,
        thread: Arc<Mutex<Thread>>,
    }

    thread_local! {
        static MINE: RefCell<Option<Mine>> = const { RefCell::new(None) };
    }

    /// One finished span.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Event {
        pub name: &'static str,
        /// Microseconds from the timeline's start.
        pub start_us: u64,
        pub dur_us: u32,
        /// How many spans it ran inside on its thread.
        pub depth: u8,
    }

    /// The spans one thread finished since the last drain, in the order they finished.
    #[derive(Debug, PartialEq, Eq)]
    pub struct ThreadEvents {
        /// The thread's name; threads alive together under one name get `#2`, `#3`... after it.
        pub thread: String,
        pub events: Vec<Event>,
    }

    /// Whether a timeline is being recorded.
    #[inline]
    pub fn on() -> bool {
        ON.load(Ordering::Relaxed)
    }

    /// Starts recording, timed from `origin`; false when a timeline already runs (the process has one).
    pub fn start(origin: Instant) -> bool {
        let mut run = RUN.lock().unwrap_or_else(PoisonError::into_inner);
        if run.is_some() {
            return false;
        }
        *run = Some(Run {
            origin,
            threads: Vec::new(),
        });
        GENERATION.fetch_add(1, Ordering::Relaxed);
        ON.store(true, Ordering::Relaxed);
        true
    }

    /// Takes what the threads finished since the last call.
    pub fn drain() -> Vec<ThreadEvents> {
        let run = RUN.lock().unwrap_or_else(PoisonError::into_inner);
        run.as_ref().map_or_else(Vec::new, take_all)
    }

    /// Stops recording and returns what the threads finished since the last drain.
    pub fn finish() -> Vec<ThreadEvents> {
        ON.store(false, Ordering::Relaxed);
        let run = RUN.lock().unwrap_or_else(PoisonError::into_inner).take();
        run.as_ref().map_or_else(Vec::new, take_all)
    }

    fn take_all(run: &Run) -> Vec<ThreadEvents> {
        run.threads
            .iter()
            .filter_map(|thread| {
                let mut thread = thread.lock().unwrap_or_else(PoisonError::into_inner);
                (!thread.events.is_empty()).then(|| ThreadEvents {
                    thread: thread.label.clone(),
                    events: std::mem::take(&mut thread.events),
                })
            })
            .collect()
    }

    /// Takes a buffer for this thread: the one of an exited thread of the same name, else a new one.
    fn join() -> Option<Mine> {
        let mut slot = RUN.lock().unwrap_or_else(PoisonError::into_inner);
        let run = slot.as_mut()?;
        let name = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        let of_name =
            |t: &Arc<Mutex<Thread>>| t.lock().unwrap_or_else(PoisonError::into_inner).name == name;
        let thread = match run
            .threads
            .iter()
            .find(|t| Arc::strong_count(t) == 1 && of_name(t))
        {
            Some(exited) => Arc::clone(exited),
            None => {
                let same = run.threads.iter().filter(|t| of_name(t)).count();
                let label = if same == 0 {
                    name.clone()
                } else {
                    format!("{name} #{}", same + 1)
                };
                let thread = Arc::new(Mutex::new(Thread {
                    name,
                    label,
                    events: Vec::new(),
                }));
                run.threads.push(Arc::clone(&thread));
                thread
            }
        };
        Some(Mine {
            generation: GENERATION.load(Ordering::Relaxed),
            origin: run.origin,
            thread,
        })
    }

    /// A span of this thread from `started`, `elapsed` long, inside `depth` others.
    pub(super) fn span(name: &'static str, started: Instant, elapsed: Duration, depth: u8) {
        if !on() {
            return;
        }
        let generation = GENERATION.load(Ordering::Relaxed);
        let _ = MINE.try_with(|mine| {
            let mut mine = mine.borrow_mut();
            if mine.as_ref().is_none_or(|m| m.generation != generation) {
                *mine = join();
            }
            let Some(mine) = mine.as_ref() else {
                return;
            };
            let event = Event {
                name,
                start_us: started.saturating_duration_since(mine.origin).as_micros() as u64,
                dur_us: elapsed.as_micros().min(u128::from(u32::MAX)) as u32,
                depth,
            };
            let mut thread = mine.thread.lock().unwrap_or_else(PoisonError::into_inner);
            if thread.events.len() < THREAD_EVENTS_MAX {
                thread.events.push(event);
            }
        });
    }
}

/// The start's timeline: `HORIZON_STARTUP_TRACE=FILE` writes every named step of
/// the start (resource tables, the renderer, the title, a world's open, a
/// server's join, a dimension change) on whichever thread ran it, one Chrome
/// trace event per line (`tools/startup_trace.py` reads it; wrapped in `[...]`,
/// Perfetto opens it). Times are microseconds since the process started. Off,
/// a step costs one relaxed load.
pub mod startup {
    use std::{
        io::Write as _,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicBool, AtomicU32, Ordering},
        },
        time::Instant,
    };

    static ON: AtomicBool = AtomicBool::new(false);
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    static NEXT_THREAD: AtomicU32 = AtomicU32::new(1);
    thread_local! {
        /// This thread's trace id, given (with its name) on its first event.
        static THREAD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    /// Opens the trace named by `HORIZON_STARTUP_TRACE`, timed from `origin`
    /// (the process's start). Call once, first thing.
    pub fn init(origin: Instant) {
        ORIGIN.get_or_init(|| origin);
        let sink = SINK.get_or_init(|| {
            let path = std::env::var_os("HORIZON_STARTUP_TRACE")?;
            std::fs::File::create(&path).ok().map(Mutex::new)
        });
        ON.store(sink.is_some(), Ordering::Relaxed);
    }

    /// Whether a trace is being written.
    #[inline]
    pub fn on() -> bool {
        ON.load(Ordering::Relaxed)
    }

    fn micros(at: Instant) -> u128 {
        ORIGIN.get().map_or(0, |origin| {
            at.saturating_duration_since(*origin).as_micros()
        })
    }

    fn write(event: std::fmt::Arguments) {
        let Some(Some(sink)) = SINK.get() else {
            return;
        };
        let tid = THREAD.with(|id| {
            if id.get() == 0 {
                id.set(NEXT_THREAD.fetch_add(1, Ordering::Relaxed));
                let name = std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .replace(['"', '\\'], "_");
                let mut file = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let _ = writeln!(
                    file,
                    r#"{{"ph":"M","name":"thread_name","pid":1,"tid":{},"args":{{"name":"{name}"}}}}"#,
                    id.get()
                );
            }
            id.get()
        });
        let mut file = sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(file, r#"{{"pid":1,"tid":{tid},{event}}}"#);
    }

    /// A step running until it is dropped.
    #[must_use = "the step ends when this is dropped"]
    pub struct Step {
        name: &'static str,
        started: Instant,
        traced: bool,
        /// A heap-census build's tag for the step's allocations.
        _heap: Option<horizon_threads::heap_census::Tag>,
    }
    impl Drop for Step {
        fn drop(&mut self) {
            if !self.traced {
                return;
            }
            let end = Instant::now();
            write(format_args!(
                r#""ph":"X","name":"{}","ts":{},"dur":{}"#,
                self.name,
                micros(self.started),
                end.saturating_duration_since(self.started).as_micros()
            ));
        }
    }

    /// Starts a step named `name` (a literal: no quotes or backslashes).
    #[inline]
    pub fn step(name: &'static str) -> Option<Step> {
        let traced = on();
        (traced || horizon_threads::HEAP_CENSUS).then(|| Step {
            name,
            started: Instant::now(),
            traced,
            _heap: horizon_threads::heap_tag(name),
        })
    }

    /// Runs `f` as a step named `name`.
    #[inline]
    pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
        let _step = step(name);
        f()
    }

    /// A moment of the start: the title drawn, the world in view.
    #[inline]
    pub fn mark(name: &'static str) {
        if on() {
            write(format_args!(
                r#""ph":"i","s":"g","name":"{name}","ts":{}"#,
                micros(Instant::now())
            ));
        }
    }

    /// The first time `name` happens, as a mark. `once` is the caller's flag.
    #[inline]
    pub fn mark_once(once: &AtomicBool, name: &'static str) {
        if on() && !once.swap(true, Ordering::Relaxed) {
            mark(name);
        }
    }

    /// Set while a world opens, a server is joined or the dimension changes,
    /// until the view first fills ([`view_filled`]).
    static FILLING: AtomicBool = AtomicBool::new(false);

    /// A world starts loading: the next full view is marked.
    pub fn arm(name: &'static str) {
        if on() {
            mark(name);
            FILLING.store(true, Ordering::Relaxed);
        }
    }

    /// Whether a world's view is still filling (between [`arm`] and the
    /// `world.view_full` mark).
    #[inline]
    pub fn filling() -> bool {
        on() && FILLING.load(Ordering::Relaxed)
    }

    /// Values at this moment, as a counter track named `name` (Perfetto draws
    /// each value as a line): what the view waits on while it fills.
    pub fn counters(name: &'static str, values: &[(&'static str, usize)]) {
        if !on() {
            return;
        }
        let args: Vec<String> = values
            .iter()
            .map(|(k, v)| format!(r#""{k}":{v}"#))
            .collect();
        write(format_args!(
            r#""ph":"C","name":"{name}","ts":{},"args":{{{}}}"#,
            micros(Instant::now()),
            args.join(",")
        ));
    }

    /// Called each frame in a world: marks `world.view_full` the first frame
    /// after [`arm`] where nothing waits to load, generate, light, mesh or
    /// upload and the resident columns cover perf_bench's share of the view
    /// (0.8 pi r^2: corners are never loaded).
    #[inline]
    pub fn view_filled(resident: usize, pending: usize, radius: u32) {
        if FILLING.load(Ordering::Relaxed)
            && pending == 0
            && resident as f64 >= 0.8 * std::f64::consts::PI * f64::from(radius * radius)
            && FILLING.swap(false, Ordering::Relaxed)
        {
            mark("world.view_full");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Span;
    use std::sync::{Mutex, MutexGuard};

    /// Spans on for every thread ([`super::enable`]) is process-wide: a test that turns them on
    /// would make another, run beside it, record what it expects left out. Those tests take
    /// this lock.
    fn spans_switch() -> MutexGuard<'static, ()> {
        static SWITCH: Mutex<()> = Mutex::new(());
        SWITCH.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Samples taken inside a named span carry its id, with spans off, and the span is left after it.
    #[cfg(target_os = "linux")]
    #[test]
    fn samples_inside_a_span_carry_its_id() {
        use horizon_threads::{sampler, work};
        use std::time::{Duration, Instant};
        let worker = std::thread::spawn(|| {
            assert!(sampler::sampled_thread("spans-id-test"));
            let config = sampler::Config {
                hz: 1000,
                ..Default::default()
            };
            let session = sampler::start(config, |n| n == "spans-id-test").unwrap();
            let busy = |ms| {
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    std::hint::black_box(0);
                }
            };
            super::time("spans_id_test_named", || busy(60));
            assert_eq!(sampler::span(), 0);
            busy(60);
            let mut spans = Vec::new();
            session.stop(|_, r| spans.push(r.ctx.span));
            spans
        });
        let spans = worker.join().unwrap();
        let inside = |s: &u16| work::name(work::Space::Span, *s) == Some("spans_id_test_named");
        let (named, plain) = (
            spans.iter().filter(|s| inside(s)).count(),
            spans.iter().filter(|s| **s == 0).count(),
        );
        assert!(
            named > 30 && plain > 30,
            "{named} named, {plain} outside of {} samples",
            spans.len()
        );
        assert_eq!(named + plain, spans.len(), "no other span id");
    }

    /// A slow tick's notes are its slowest steps, slowest first; short steps and steps on a
    /// thread that records no spans leave none.
    #[test]
    fn notes_keep_the_slowest_steps() {
        let _switch = spans_switch();
        super::record_here(false);
        super::note(500.0, || "unrecorded".into());
        assert!(super::take_notes().is_empty());
        super::record_here(true);
        for (ms, what) in [
            (12.0, "a"),
            (900.0, "b"),
            (3.0, "short"),
            (40.0, "c"),
            (11.0, "d"),
            (60.0, "e"),
        ] {
            super::note(ms, || what.into());
        }
        let notes: Vec<(f32, String)> = super::take_notes();
        super::record_here(false);
        let names: Vec<&str> = notes.iter().map(|(_, w)| w.as_str()).collect();
        assert_eq!(names, ["b", "e", "c"]);
        assert!(super::take_notes().is_empty(), "taking them clears them");
    }

    /// Spans of one name add up, the slowest first; a span that waited keeps its small CPU time.
    #[test]
    fn spans_sum_by_name_and_show_waits() {
        super::SPANS.with(|s| {
            s.borrow_mut().extend([
                Span {
                    name: "tick",
                    ms: 2.0,
                    cpu_ms: 2.0,
                    depth: 0,
                },
                Span {
                    name: "submit",
                    ms: 30.0,
                    cpu_ms: 0.2,
                    depth: 0,
                },
                Span {
                    name: "tick",
                    ms: 3.0,
                    cpu_ms: 2.9,
                    depth: 0,
                },
                Span {
                    name: "hud",
                    ms: 0.3,
                    cpu_ms: 0.3,
                    depth: 0,
                },
            ]);
        });
        let spans = super::take();
        assert_eq!(
            spans[0],
            Span {
                name: "submit",
                ms: 30.0,
                cpu_ms: 0.2,
                depth: 0,
            }
        );
        assert_eq!((spans[1].name, spans[1].ms), ("tick", 5.0));
        assert!(super::take().is_empty());
    }

    /// A thread that records on its own keeps its spans with spans off, each with how many
    /// spans it ran inside, after them.
    #[test]
    fn a_recording_thread_keeps_its_spans_and_their_nesting() {
        std::thread::spawn(|| {
            super::record_here(true);
            super::time("tick", || {
                super::time("tick_mobs", || ());
                super::time("tick_items", || ());
            });
            super::time("renderer", || ());
            let raw = super::take_raw();
            let seen: Vec<_> = raw.iter().map(|s| (s.name, s.depth)).collect();
            assert_eq!(
                seen,
                [
                    ("tick_mobs", 1),
                    ("tick_items", 1),
                    ("tick", 0),
                    ("renderer", 0)
                ]
            );
        })
        .join()
        .unwrap();
    }

    /// A guarded span ends on every way out of its step, an early return's too, and holds the
    /// spans that ran inside it until then; the next span outside it is back at its depth.
    #[test]
    fn a_guarded_span_ends_where_its_step_returns() {
        fn step(early: bool) -> u32 {
            let _span = super::guard("use_block");
            super::time("open_loot", || ());
            if early {
                return 1;
            }
            super::time("open_maps", || ());
            2
        }
        std::thread::spawn(|| {
            super::record_here(true);
            assert_eq!(step(true), 1);
            assert_eq!(step(false), 2);
            super::time("use_item", || ());
            let raw = super::take_raw();
            let seen: Vec<_> = raw.iter().map(|s| (s.name, s.depth)).collect();
            assert_eq!(
                seen,
                [
                    ("open_loot", 1),
                    ("use_block", 0),
                    ("open_loot", 1),
                    ("open_maps", 1),
                    ("use_block", 0),
                    ("use_item", 0)
                ]
            );
        })
        .join()
        .unwrap();
    }

    /// A slow frame is put down to the slowest outer span, then to the span inside it that took
    /// at least half of it; repeated spans (several ticks a frame) add up.
    #[test]
    fn a_frame_is_put_down_to_the_step_that_took_most_of_it() {
        let span = |name, ms, depth| Span {
            name,
            ms,
            cpu_ms: ms,
            depth,
        };
        let frame = [
            span("tick_mobs", 90.0, 1),
            span("tick_items", 5.0, 1),
            span("tick", 100.0, 0),
            span("tick_mobs", 90.0, 1),
            span("tick", 100.0, 0),
            span("r_uploads", 120.0, 1),
            span("renderer", 150.0, 0),
        ];
        assert_eq!(
            super::culprit(&frame),
            Some(("tick/tick_mobs".to_owned(), 180.0))
        );
        // Spread over many steps: the outer span alone.
        let spread = [
            span("tick_mobs", 40.0, 1),
            span("tick_items", 40.0, 1),
            span("tick", 300.0, 0),
        ];
        assert_eq!(super::culprit(&spread), Some(("tick".to_owned(), 300.0)));
        assert_eq!(super::culprit(&[]), None);
    }

    /// Laps record the steps between them while spans are on, and nothing while off.
    #[test]
    fn laps_time_consecutive_steps() {
        let _switch = spans_switch();
        let _ = super::take();
        let mut off = super::laps();
        off.lap("unseen");
        super::enable(true);
        let mut laps = super::laps();
        std::thread::sleep(std::time::Duration::from_millis(2));
        laps.lap("first");
        laps.lap("second");
        super::enable(false);
        off.lap("still unseen");
        let spans = super::take();
        let names: Vec<_> = spans.iter().map(|s| s.name).collect();
        assert_eq!(names, ["first", "second"]);
        assert!(spans[0].ms >= 2.0);
    }

    /// A span that ran during a lap belongs inside that lap, and not inside the next one.
    #[test]
    fn a_span_during_a_lap_nests_under_that_lap() {
        let _switch = spans_switch();
        let _ = super::take_raw();
        super::enable(true);
        let mut laps = super::laps();
        super::time("inner", || {});
        laps.lap("first");
        laps.lap("second");
        super::enable(false);
        let spans = super::take_raw();
        let depths: Vec<_> = spans.iter().map(|s| (s.name, s.depth)).collect();
        assert_eq!(depths, [("inner", 1), ("first", 0), ("second", 0)]);
    }

    /// The thread's CPU clock moves with work done on it, not with time
    /// spent asleep.
    #[cfg(any(unix, all(windows, target_arch = "x86_64")))]
    #[test]
    fn thread_cpu_counts_work_not_sleep() {
        let start = super::thread_cpu().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let slept = super::thread_cpu().unwrap() - start;
        // Busy until the clock shows 10 ms of work (a loaded machine gives
        // the thread less than its wall time).
        let mut x = 0u64;
        let busy = std::time::Instant::now();
        let worked = loop {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
            let worked = super::thread_cpu().unwrap() - start - slept;
            if worked.as_millis() >= 10 || busy.elapsed().as_secs() >= 10 {
                break worked;
            }
        };
        assert!(slept.as_millis() < 25, "{slept:?} while asleep");
        assert!(worked.as_millis() >= 10, "{worked:?} while busy");
    }

    /// Each thread's finished spans reach the timeline with their depth and nesting in time; threads alive together under one name get labels of their own, ones after another share a lane.
    #[test]
    fn the_timeline_keeps_each_threads_spans_with_their_depth_and_place_in_time() {
        use super::timeline;
        use std::sync::{Arc, Barrier};
        use std::time::{Duration, Instant};
        let _switch = spans_switch();
        assert!(timeline::start(Instant::now()));
        assert!(
            !timeline::start(Instant::now()),
            "the process has one timeline"
        );
        let spawn = |name: &str, alive_with: Option<Arc<Barrier>>| {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    super::time("outer", || {
                        std::thread::sleep(Duration::from_millis(4));
                        super::time("inner", || std::thread::sleep(Duration::from_millis(3)));
                    });
                    if let Some(barrier) = alive_with {
                        barrier.wait();
                    }
                })
                .unwrap()
        };
        spawn("tl-solo", None).join().unwrap();
        let together = Arc::new(Barrier::new(2));
        let twins = [
            spawn("tl-twin", Some(together.clone())),
            spawn("tl-twin", Some(together)),
        ];
        for twin in twins {
            twin.join().unwrap();
        }
        spawn("tl-seq", None).join().unwrap();
        spawn("tl-seq", None).join().unwrap();
        let mut threads = timeline::finish();
        threads.retain(|t| t.thread.starts_with("tl-"));
        threads.sort_by(|a, b| a.thread.cmp(&b.thread));
        let labels: Vec<_> = threads.iter().map(|t| t.thread.as_str()).collect();
        assert_eq!(labels, ["tl-seq", "tl-solo", "tl-twin", "tl-twin #2"]);
        for thread in &threads {
            let runs = if thread.thread == "tl-seq" { 2 } else { 1 };
            assert_eq!(thread.events.len(), 2 * runs, "{thread:?}");
            for pair in thread.events.chunks(2) {
                let [inner, outer] = pair else { unreachable!() };
                assert_eq!((inner.name, inner.depth), ("inner", 1));
                assert_eq!((outer.name, outer.depth), ("outer", 0));
                assert!(
                    inner.dur_us >= 3_000 && outer.dur_us >= 7_000,
                    "{inner:?} {outer:?}"
                );
                assert!(inner.start_us >= outer.start_us + 4_000);
                assert!(
                    inner.start_us + u64::from(inner.dur_us)
                        <= outer.start_us + u64::from(outer.dur_us)
                );
            }
        }
        let seq = threads.iter().find(|t| t.thread == "tl-seq").unwrap();
        assert!(seq.events[2].start_us >= seq.events[1].start_us + u64::from(seq.events[1].dur_us));

        spawn("tl-after", None).join().unwrap();
        assert!(
            timeline::drain().is_empty(),
            "a stopped timeline records nothing"
        );
    }
}
