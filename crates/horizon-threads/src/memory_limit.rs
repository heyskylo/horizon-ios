//! The player's cap on the memory the game holds, and the last resort behind it.
//!
//! On every system the game keeps itself under the cap by holding less, before it reaches it
//! (`horizon_client::ram_limit`, which reads [`pressure`]). Nothing asks the system to page the
//! game out: on Windows a hard working set limit did, and a low cap made the game slow instead of
//! making it hold less.
//!
//! A watcher thread reads the process's resident memory and, over the cap, gives free pages back.
//! Only when the game has nothing left to give up ([`can_shrink`] has stopped being called) and
//! stays over for [`GRACE`] does it call the game's `exceeded`. No thread runs until a cap is set,
//! and there is no lowest cap: one the game can't live under ends the game.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, Once, OnceLock};
use std::time::{Duration, Instant};

const EVERY: Duration = Duration::from_secs(1);
/// How long the game may stay over the cap with nothing left to give up: a world's first chunks,
/// or a burst the last step down has not freed yet, pass within it.
pub const GRACE: Duration = Duration::from_secs(20);
/// How long one [`can_shrink`] counts for.
const SHRINK_WORD: Duration = Duration::from_secs(5);

#[derive(Default)]
struct WatchConfig {
    limit: Option<u64>,
    suspended: bool,
    revision: u64,
}
impl WatchConfig {
    fn active(&self) -> Option<u64> {
        self.limit.filter(|_| !self.suspended)
    }
}

#[derive(Default)]
struct Monitor {
    config: Mutex<WatchConfig>,
    changed: Condvar,
}
impl Monitor {
    fn change(&self, update: impl FnOnce(&mut WatchConfig), apply: impl FnOnce(Option<u64>)) {
        let mut config = self.config.lock().unwrap_or_else(|e| e.into_inner());
        update(&mut config);
        config.revision = config.revision.wrapping_add(1);
        apply(config.active());
        self.changed.notify_all();
    }

    /// Sleeps indefinitely without a cap, and once per interval with one. The
    /// same lock protects changes and sampling, so suspension waits for a check
    /// already in progress and no stale check can fire after it returns.
    fn next_sample(&self, interval: Duration) -> MutexGuard<'_, WatchConfig> {
        let mut config = self.config.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            config = self
                .changed
                .wait_while(config, |c| c.active().is_none())
                .unwrap_or_else(|e| e.into_inner());
            let revision = config.revision;
            let (next, timeout) = self
                .changed
                .wait_timeout_while(config, interval, |c| c.revision == revision)
                .unwrap_or_else(|e| e.into_inner());
            config = next;
            if timeout.timed_out() && config.active().is_some() {
                return config;
            }
        }
    }
}
static MONITOR: Monitor = Monitor {
    config: Mutex::new(WatchConfig {
        limit: None,
        suspended: false,
        revision: 0,
    }),
    changed: Condvar::new(),
};
static WATCHER: Once = Once::new();
static STARTED: OnceLock<Instant> = OnceLock::new();
/// Until when, in milliseconds since [`STARTED`], the game says it can still shrink.
static SHRINKS_UNTIL: AtomicU64 = AtomicU64::new(0);

fn since_start() -> u64 {
    STARTED.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The game can still hold less. Said again every second while that is true, so it ends by itself
/// when the world closes.
pub fn can_shrink() {
    let until = since_start() + SHRINK_WORD.as_millis() as u64;
    SHRINKS_UNTIL.store(until, Ordering::Relaxed);
}

/// Sets the cap in bytes, or removes it. `exceeded(used, limit)` runs on the watcher thread when
/// the game can't get under the cap; only the first call's function is kept.
pub fn set(limit: Option<u64>, exceeded: fn(u64, u64)) {
    MONITOR.change(|config| config.limit = limit, |_| {});
    if limit.is_some() {
        WATCHER.call_once(|| {
            let watcher = std::thread::Builder::new()
                .name("Memory limit".into())
                .spawn(move || {
                    let mut over = Over::default();
                    let mut revision = None;
                    loop {
                        let config = MONITOR.next_sample(EVERY);
                        if revision != Some(config.revision) {
                            over = Over::default();
                            revision = Some(config.revision);
                        }
                        let limit = config.active();
                        over.follow(limit);
                        if let Some(limit) = limit {
                            check(&mut over, limit, exceeded);
                        }
                    }
                });
            if let Err(e) = watcher {
                eprintln!("The memory limit can't be watched: {e}");
            }
        });
    }
}

/// The cap in bytes, if one is set.
pub fn get() -> Option<u64> {
    MONITOR
        .config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active()
}

/// The player's configured cap and whether enforcement is temporarily suspended.
/// Unlike `get`, this preserves the value while the launcher is open.
pub fn configuration() -> (Option<u64>, bool) {
    let config = MONITOR.config.lock().unwrap_or_else(|e| e.into_inner());
    (config.limit, config.suspended)
}

/// Temporarily stop enforcement and sampling while a mobile launcher owns the
/// process. The configured cap (including an in-game change) is retained.
pub fn suspend(suspended: bool) {
    MONITOR.change(|config| config.suspended = suspended, |_| {});
}

/// How tight memory is for the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pressure {
    /// It should hold less; `keep` is the share of what it holds to aim for.
    Over {
        keep: f64,
    },
    /// It could hold more.
    Room,
    Steady,
}

/// What the game holds against the cap: it should act before it gets there.
fn pressure_of_usage(used: u64, limit: u64) -> Pressure {
    if used > limit / 10 * 9 {
        Pressure::Over {
            keep: limit as f64 * 0.8 / used as f64,
        }
    } else if used < limit / 10 * 7 {
        Pressure::Room
    } else {
        Pressure::Steady
    }
}

/// Against a hard ceiling, what the process commits is the budget, not what stays in RAM: a
/// ceiling counts every page the process committed, the renderer's own included.
pub fn pressure_of_commit(commit: u64, limit: u64) -> Pressure {
    if commit > limit / 10 * 9 {
        Pressure::Over {
            keep: limit as f64 * 0.8 / commit as f64,
        }
    } else if commit < limit / 10 * 7 {
        Pressure::Room
    } else {
        Pressure::Steady
    }
}

/// How tight memory is under `limit`: what the game holds in RAM against it.
pub fn pressure(limit: u64) -> Pressure {
    resident_bytes().map_or(Pressure::Steady, |used| pressure_of_usage(used, limit))
}

/// How long the game has been over the cap with nothing left to give up.
#[derive(Default)]
struct Over {
    since: Option<Instant>,
    /// The cap the grace is counted against.
    limit: Option<u64>,
}

impl Over {
    /// A cap turned off, or set to another value, starts its grace again.
    fn follow(&mut self, limit: Option<u64>) {
        if limit != self.limit {
            self.since = None;
            self.limit = limit;
        }
    }
    /// Whether this reading ends the game.
    fn gave_up(&mut self, now: Instant, over: bool, can_shrink: bool) -> bool {
        if !over || can_shrink {
            self.since = None;
            return false;
        }
        now.duration_since(*self.since.get_or_insert(now)) >= GRACE
    }
}

fn check(state: &mut Over, limit: u64, exceeded: fn(u64, u64)) {
    // A hard ceiling counts what the process committed, the renderer's own
    // pages included, since that is the number the player set.
    let hard = crate::capped::limit().is_some();
    let measured = if hard {
        Some(crate::allocator_commit().0 as u64)
    } else {
        resident_bytes()
    };
    let Some(mut used) = measured else {
        return;
    };
    if used > limit {
        crate::release_free_memory();
        used = if hard {
            crate::allocator_commit().0 as u64
        } else {
            resident_bytes().unwrap_or(used)
        };
    }
    let can_shrink = since_start() < SHRINKS_UNTIL.load(Ordering::Relaxed);
    if state.gave_up(Instant::now(), used > limit, can_shrink) {
        exceeded(used, limit);
    }
}

/// The process's memory counters.
#[cfg(windows)]
fn counters() -> Option<windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: all-zero is a valid value of this plain C struct, and the call writes at most `size`
    // bytes into it; the current process's pseudo-handle needs no closing.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = size;
        (K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) != 0).then_some(counters)
    }
}

/// The memory this process holds in RAM, in bytes.
#[cfg(windows)]
pub fn resident_bytes() -> Option<u64> {
    Some(counters()?.WorkingSetSize as u64)
}

/// The memory this process holds in RAM, in bytes.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: reads one system constant.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(pages * u64::try_from(page).ok()?)
}

/// The memory this process holds in RAM, in bytes.
#[cfg(target_os = "macos")]
pub fn resident_bytes() -> Option<u64> {
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: all-zero is a valid value of this plain C struct, and the call writes at most `size`
    // bytes into it.
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let written = libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&raw mut info).cast(),
            size,
        );
        (written == size).then_some(info.pti_resident_size)
    }
}

/// The memory this process holds in RAM, in bytes. libproc's proc_pidinfo is
/// macOS-only; iOS and the other Apple platforms expose this through Mach.
#[cfg(all(target_vendor = "apple", not(target_os = "macos")))]
pub fn resident_bytes() -> Option<u64> {
    mach_resident_bytes()
}

#[cfg(all(target_vendor = "apple", any(not(target_os = "macos"), test)))]
fn mach_resident_bytes() -> Option<u64> {
    // mach_init.h defines mach_task_self() as this libSystem variable. libc's
    // Rust accessor is macOS-only, although the SDK exports it on iOS too.
    unsafe extern "C" {
        static mut mach_task_self_: libc::mach_port_t;
    }
    // MACH_TASK_BASIC_INFO is the SDK's architecture-independent, 64-bit form.
    // Its count is in natural_t words, not bytes (mach/task_info.h).
    let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::uninit();
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: info has space for exactly `count` natural_t words; task_info writes
    // that many at most. The current task port is borrowed and needs no deallocation.
    let result = unsafe {
        libc::task_info(
            mach_task_self_,
            libc::MACH_TASK_BASIC_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS || count != libc::MACH_TASK_BASIC_INFO_COUNT {
        return None;
    }
    // SAFETY: success with the full count initializes the SDK's plain-data struct.
    Some(unsafe { info.assume_init() }.resident_size)
}

/// The memory this process holds in RAM, in bytes: not known on this system.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
)))]
pub fn resident_bytes() -> Option<u64> {
    None
}

#[cfg(all(test, any(windows, target_os = "linux", target_vendor = "apple")))]
mod tests {
    use super::*;

    /// The working set is the whole test process's, and one test caps it.
    static WORKING_SET: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn touched_memory_counts_as_resident() {
        let _alone = WORKING_SET.lock().unwrap();
        const SIZE: usize = 256 << 20;
        let before = resident_bytes().unwrap();
        let block = vec![1u8; SIZE];
        let during = resident_bytes().unwrap();
        std::hint::black_box(&block);
        assert!(
            during >= before + SIZE as u64 / 2,
            "{before} then {during} bytes"
        );
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn mach_task_sampling_counts_touched_memory() {
        let _alone = WORKING_SET.lock().unwrap();
        const SIZE: usize = 64 << 20;
        let before = mach_resident_bytes().expect("current task memory");
        let block = vec![3u8; SIZE];
        let during = mach_resident_bytes().expect("current task memory after allocation");
        std::hint::black_box(&block);
        assert!(
            during >= before + SIZE as u64 / 2,
            "{before} then {during} bytes"
        );
    }

    #[test]
    fn watcher_parks_without_a_cap_and_wakes_when_enabled() {
        use std::sync::{Arc, mpsc};
        let monitor = Arc::new(Monitor::default());
        let worker = monitor.clone();
        let (done, result) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let sample = worker.next_sample(Duration::from_millis(5));
            done.send(sample.active()).unwrap();
        });
        let premature = result.recv_timeout(Duration::from_millis(30)).is_ok();
        monitor.change(|c| c.limit = Some(1024), |_| {});
        let sampled = result.recv_timeout(Duration::from_secs(1));
        thread.join().unwrap();
        assert!(!premature, "disabled watcher sampled instead of parking");
        assert_eq!(sampled.unwrap(), Some(1024));
    }

    #[test]
    fn suspension_retains_runtime_changes_and_resets_the_sampling_generation() {
        use std::sync::{Arc, mpsc};
        let monitor = Arc::new(Monitor::default());
        monitor.change(|c| c.limit = Some(2048), |_| {});
        let old_revision = monitor.config.lock().unwrap().revision;
        monitor.change(|c| c.suspended = true, |active| assert_eq!(active, None));
        // A setting change while suspended must not wake the sampler into enforcement.
        monitor.change(|c| c.limit = Some(4096), |active| assert_eq!(active, None));
        let worker = monitor.clone();
        let (done, result) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let sample = worker.next_sample(Duration::from_millis(5));
            done.send((sample.active(), sample.revision)).unwrap();
        });
        let premature = result.recv_timeout(Duration::from_millis(30)).is_ok();
        monitor.change(
            |c| c.suspended = false,
            |active| assert_eq!(active, Some(4096)),
        );
        let sampled = result.recv_timeout(Duration::from_secs(1));
        thread.join().unwrap();
        assert!(!premature, "suspended watcher kept sampling");
        let (cap, revision) = sampled.unwrap();
        assert_eq!(cap, Some(4096));
        assert_ne!(
            revision, old_revision,
            "old grace must not survive a launcher visit"
        );
    }

    #[test]
    fn suspension_waits_for_an_in_flight_check_before_returning() {
        use std::sync::{Arc, mpsc};
        let monitor = Arc::new(Monitor::default());
        monitor.change(|c| c.limit = Some(1024), |_| {});
        let worker = monitor.clone();
        let (checking, checked) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let sampler = std::thread::spawn(move || {
            let _sample = worker.next_sample(Duration::from_millis(1));
            checking.send(()).unwrap();
            wait.recv().unwrap();
        });
        checked.recv_timeout(Duration::from_secs(1)).unwrap();
        let setter = monitor.clone();
        let (done, result) = mpsc::channel();
        let suspended = std::thread::spawn(move || {
            setter.change(|c| c.suspended = true, |_| {});
            done.send(()).unwrap();
        });
        let premature = result.recv_timeout(Duration::from_millis(30)).is_ok();
        release.send(()).unwrap();
        sampler.join().unwrap();
        suspended.join().unwrap();
        assert!(
            !premature,
            "suspension returned while a stale check could still fire"
        );
        assert!(monitor.config.lock().unwrap().active().is_none());
    }

    #[test]
    fn usage_says_how_tight_a_cap_nothing_enforces_is() {
        const GB: u64 = 1 << 30;
        assert_eq!(pressure_of_usage(GB / 2, GB), Pressure::Room);
        assert_eq!(pressure_of_usage(GB / 10 * 8, GB), Pressure::Steady);
        let Pressure::Over { keep } = pressure_of_usage(2 * GB, GB) else {
            panic!("twice the cap is over it");
        };
        assert!((keep - 0.4).abs() < 0.001, "{keep}");
    }

    #[test]
    fn every_system_steps_down_before_its_cap() {
        let _alone = WORKING_SET.lock().unwrap();
        let used = resident_bytes().unwrap();
        // Under a cap at half what it holds, the game is told to hold less, on Windows as
        // elsewhere: nothing pages it out to meet the cap instead.
        let Pressure::Over { keep } = pressure(used / 2) else {
            panic!("holding {used} bytes is over a cap of half that");
        };
        assert!(keep < 0.5, "{keep}");
        // Far below the cap it may hold more again.
        assert_eq!(pressure(used.saturating_mul(4)), Pressure::Room);
    }

    #[test]
    fn only_staying_over_with_nothing_left_to_give_up_ends_the_game() {
        let start = Instant::now();
        let later = |d: Duration| start + d;
        let mut over = Over::default();
        assert!(!over.gave_up(start, true, false));
        assert!(!over.gave_up(later(GRACE / 2), true, false));
        assert!(over.gave_up(later(GRACE), true, false));

        // Getting back under the cap starts the count again.
        let mut over = Over::default();
        assert!(!over.gave_up(start, true, false));
        assert!(!over.gave_up(later(GRACE / 2), false, false));
        assert!(!over.gave_up(later(GRACE), true, false));

        // So does having something left to give up, however long the game is over.
        let mut over = Over::default();
        assert!(!over.gave_up(start, true, false));
        assert!(!over.gave_up(later(GRACE * 5), true, true));
        assert!(!over.gave_up(later(GRACE * 6), true, false));

        // A cap turned off, then on again, or changed, gets its whole grace again.
        let mut over = Over::default();
        over.follow(Some(1 << 30));
        assert!(!over.gave_up(start, true, false));
        over.follow(None);
        over.follow(Some(1 << 30));
        assert!(!over.gave_up(later(GRACE), true, false));
        over.follow(Some(1 << 29));
        assert!(!over.gave_up(later(GRACE * 2), true, false));
        over.follow(Some(1 << 29));
        assert!(over.gave_up(later(GRACE * 3), true, false));
    }

    #[test]
    fn saying_the_game_can_shrink_holds_the_last_resort_off_for_a_while() {
        can_shrink();
        assert!(since_start() < SHRINKS_UNTIL.load(Ordering::Relaxed));
    }

    #[test]
    fn a_commit_budget_says_over_near_it_and_room_far_below() {
        assert_eq!(
            pressure_of_commit(950, 1000),
            Pressure::Over {
                keep: 0.8 * 1000. / 950.
            }
        );
        assert_eq!(pressure_of_commit(800, 1000), Pressure::Steady);
        assert_eq!(pressure_of_commit(600, 1000), Pressure::Room);
    }

    #[test]
    fn a_commit_budget_aims_for_less_the_further_over_it_is() {
        let a = pressure_of_commit(1000, 1000);
        let b = pressure_of_commit(2000, 1000);
        let (Pressure::Over { keep: a }, Pressure::Over { keep: b }) = (a, b) else {
            panic!("both are over");
        };
        assert!(a > b, "twice the budget asks for half as much");
        assert!((b - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_budget_of_zero_never_says_room() {
        assert!(matches!(pressure_of_commit(1, 0), Pressure::Over { .. }));
    }
}
