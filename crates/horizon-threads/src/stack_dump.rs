//! Another thread's stack, read from outside it: the server watchdog's crash report says what
//! the hung tick was doing, as vanilla's `ServerWatchdog` dumps the server thread's stack (#15349).
//!
//! On Linux, Android and Apple systems a signal interrupts the watched thread and its handler
//! captures that thread's own backtrace. The handler allocates, which a signal handler should not
//! do: ask only right before the process ends, as the watchdog does. A thread that never runs the
//! handler (stuck in the kernel, or deadlocked inside the handler) leaves the asker without an
//! answer once its wait is over.

use std::time::Duration;

/// A thread whose stack another thread can read ([`Watched::stack`]).
#[derive(Clone, Copy, Debug)]
pub struct Watched {
    #[cfg_attr(
        not(any(target_vendor = "apple", target_os = "linux", target_os = "android")),
        allow(dead_code)
    )]
    thread: usize,
}

/// Why [`Watched::stack`] has no stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoStack {
    /// This system can't interrupt another thread to read its stack.
    Unsupported,
    /// The thread did not answer within the wait (blocked in the kernel, for one).
    NoAnswer,
}

impl std::fmt::Display for NoStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NoStack::Unsupported => "not available on this system",
            NoStack::NoAnswer => "the thread did not answer",
        })
    }
}

impl Watched {
    /// The calling thread. It must still run when [`Watched::stack`] is asked.
    pub fn current() -> Self {
        Self {
            #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
            thread: unsafe { libc::pthread_self() } as usize,
            #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
            thread: 0,
        }
    }

    /// The thread's stack as `std::backtrace::Backtrace` prints it, starting at the frame the
    /// thread was in (on Apple's arm64, at its caller), waiting at most `wait` for it.
    pub fn stack(&self, wait: Duration) -> Result<String, NoStack> {
        #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
        {
            imp::stack(self.thread, wait).map(|b| after_handler(&b.to_string()))
        }
        #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
        {
            let _ = wait;
            Err(NoStack::Unsupported)
        }
    }
}

/// A backtrace's text without the frames the signal added: the capture's, the handler's and the
/// signal trampoline's, the one frame the system puts between the handler and the interrupted
/// code (named `__restore_rt`, `_sigtramp`, or after whatever symbol precedes it). Unchanged when
/// the handler's frame isn't found.
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "linux", target_os = "android")),
    allow(dead_code)
)]
fn after_handler(text: &str) -> String {
    fn frame_name(line: &str) -> Option<&str> {
        let (n, name) = line.trim().split_once(": ")?;
        n.chars().all(|c| c.is_ascii_digit()).then_some(name)
    }
    let lines: Vec<&str> = text.lines().collect();
    let frames: Vec<usize> = (0..lines.len())
        .filter(|&i| frame_name(lines[i]).is_some())
        .collect();
    let Some(handler) = frames.iter().position(|&i| {
        frame_name(lines[i]).is_some_and(|n| n == "on_signal" || n.ends_with("::on_signal"))
    }) else {
        return text.to_owned();
    };
    let start = frames.get(handler + 2).copied().unwrap_or(lines.len());
    lines[start..].iter().map(|l| format!("{l}\n")).collect()
}

#[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
mod imp {
    use super::NoStack;
    use std::{
        backtrace::Backtrace,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicPtr, Ordering},
        },
        time::{Duration, Instant},
    };

    /// Ignored when no handler is installed, and used by nothing else here (Go's runtime takes it
    /// for the same reason): a stray one does nothing.
    const SIGNAL: libc::c_int = libc::SIGURG;

    /// The handler's answer, taken by the asker.
    static ANSWER: AtomicPtr<Backtrace> = AtomicPtr::new(std::ptr::null_mut());
    /// One question at a time.
    static ASKING: Mutex<()> = Mutex::new(());

    extern "C" fn on_signal(_: libc::c_int) {
        let stack = Box::into_raw(Box::new(Backtrace::force_capture()));
        // An answer nobody took (its asker gave up) is left as it is: freeing here could be
        // the allocation that goes wrong.
        ANSWER.store(stack, Ordering::Release);
    }

    fn installed() -> bool {
        static INSTALLED: OnceLock<bool> = OnceLock::new();
        *INSTALLED.get_or_init(|| unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(SIGNAL, &action, std::ptr::null_mut()) == 0
        })
    }

    fn take() -> Option<Backtrace> {
        let answer = ANSWER.swap(std::ptr::null_mut(), Ordering::Acquire);
        // SAFETY: only `on_signal` stores, a pointer from `Box::into_raw`, and the swap takes it once.
        (!answer.is_null()).then(|| *unsafe { Box::from_raw(answer) })
    }

    pub(super) fn stack(thread: usize, wait: Duration) -> Result<Backtrace, NoStack> {
        let _one = ASKING.lock().unwrap_or_else(|e| e.into_inner());
        if !installed() {
            return Err(NoStack::Unsupported);
        }
        // A late answer to an earlier question.
        drop(take());
        // SAFETY: `Watched::current`'s contract keeps the thread alive.
        if unsafe { libc::pthread_kill(thread as libc::pthread_t, SIGNAL) } != 0 {
            return Err(NoStack::NoAnswer);
        }
        let until = Instant::now() + wait;
        loop {
            if let Some(stack) = take() {
                return Ok(stack);
            }
            if Instant::now() >= until {
                return Err(NoStack::NoAnswer);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The busy thread below serves the test that reads another thread's stack, which only the
    // systems with a signal path run.
    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    #[inline(never)]
    fn spin(mut n: u64) -> u64 {
        for _ in 0..1_000_000 {
            n = std::hint::black_box(n.wrapping_mul(31).wrapping_add(7));
        }
        n
    }

    /// Its time goes to a call: Apple's arm64 unwinder loses the frame the signal interrupted
    /// (it resumes from the return address), while every caller's frame stays.
    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    #[inline(never)]
    fn busy_in_a_named_frame(stop: &AtomicBool) {
        let mut n = 0u64;
        while !stop.load(Ordering::Relaxed) {
            n = std::hint::black_box(spin)(n);
        }
    }

    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    #[test]
    fn reads_the_stack_of_a_busy_thread_from_another() {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                tx.send(Watched::current()).unwrap();
                busy_in_a_named_frame(&stop);
            })
        };
        let watched = rx.recv().unwrap();
        let stack = watched.stack(Duration::from_secs(10));
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        let stack = stack.unwrap();
        let busy = stack.find("busy_in_a_named_frame");
        assert!(
            busy.is_some(),
            "the busy thread's frame is in its stack:\n{stack}"
        );
        assert!(
            !stack[..busy.unwrap()].contains("on_signal") && !stack.contains("Backtrace"),
            "the stack starts where the thread was, not in the signal handler:\n{stack}"
        );
    }

    #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
    #[test]
    fn says_when_the_system_cannot_read_a_stack() {
        assert_eq!(
            Watched::current().stack(Duration::from_millis(10)),
            Err(NoStack::Unsupported)
        );
    }

    #[test]
    fn drops_the_capture_handler_and_trampoline_frames() {
        let text = "   0: std::backtrace::Backtrace::force_capture
             at /rustc/library/std/src/backtrace.rs:312:9
   1: horizon_threads::stack_dump::imp::on_signal
             at ./src/stack_dump.rs:120:40
   2: __os_lock_spin_lock
   3: horizon_client::game::Game::tick_server
             at ./src/game/levels.rs:10:5
   4: main
";
        assert_eq!(
            after_handler(text),
            "   3: horizon_client::game::Game::tick_server
             at ./src/game/levels.rs:10:5
   4: main
"
        );
    }
}
