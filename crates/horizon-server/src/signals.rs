//! Stop requests from outside: Ctrl-C in the terminal, SIGTERM from systemd or launchd, SIGHUP when the
//! terminal closes, the console window closing on Windows. The handler only counts them; the supervisor
//! sees the count and stops the game cleanly (a second request stops it at once). On Windows a console close,
//! logoff or shutdown ends the process when its handler returns, so that handler waits for [`stopped`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static REQUESTS: AtomicU32 = AtomicU32::new(0);

/// How many stop requests arrived.
pub fn requests() -> u32 {
    REQUESTS.load(Ordering::SeqCst)
}

#[cfg(unix)]
pub fn install() {
    extern "C" fn handle(_: libc::c_int) {
        REQUESTS.fetch_add(1, Ordering::SeqCst);
    }
    let handler = handle as extern "C" fn(libc::c_int);
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler only increments an atomic, which is async-signal-safe.
        unsafe {
            libc::signal(signal, handler as libc::sighandler_t);
        }
    }
}

static STOPPED: AtomicBool = AtomicBool::new(false);

/// The supervisor is done: its game has stopped. A console close, logoff or shutdown handler
/// waiting on it may return.
pub fn stopped() {
    STOPPED.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
pub fn install() {
    unsafe extern "system" fn handle(event: u32) -> windows_sys::core::BOOL {
        use windows_sys::Win32::System::Console::{
            CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
        };
        REQUESTS.fetch_add(1, Ordering::SeqCst);
        // For these Windows ends the process as soon as the handler returns, which would leave the
        // game (in its own process group, so it gets no event) running unsupervised with the world
        // and port. Wait for the clean stop, within what Windows allows: about 5 s after a close,
        // longer for a logoff or shutdown.
        if matches!(
            event,
            CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
        ) {
            let limit = if event == CTRL_CLOSE_EVENT { 4 } else { 15 };
            let until = std::time::Instant::now() + std::time::Duration::from_secs(limit);
            while !STOPPED.load(Ordering::SeqCst) && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        1
    }
    // SAFETY: the handler only increments an atomic; it stays valid for the life of the process.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handle), 1);
    }
}

#[cfg(not(any(unix, windows)))]
pub fn install() {}
