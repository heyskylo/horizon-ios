//! The phone's launcher, in the game's own bundle. The game owns the app (winit runs UIKit's main loop and
//! never hands it back), so the launcher cannot run before it: Xcode embeds the SwiftUI launcher as
//! `Frameworks/HorizonLauncherKit.framework`, and once the game's window is up [`present`] loads it and asks
//! it to cover the game. Its Play button uncovers the game, at its title screen.
//!
//! Loaded at run time rather than linked, so a bundle without the framework (the workflow's hand-assembled
//! one, a desktop build) is the game as before. The game forbids `unsafe`, so the loading lives here.
//!
//! The launcher talks to the game in the game's own control language (docs/horizon-ctl-commands.md): it
//! hands [`present`]'s caller a line such as `worlds` or `open My World`, and the game answers with the JSON
//! the control socket would. The two share one thread, the main one, so nothing here waits: a line is queued
//! ([`take`]), the game answers it on its next frame, and the answer goes back to Swift through the callback
//! that came with it.

use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
};

/// One line from the launcher, and the way back to it.
pub struct Request {
    pub line: String,
    reply: Option<Box<dyn FnOnce(&str) + Send>>,
}

impl Request {
    /// A request answered by `reply`; the launcher's own requests are made by [`present`]'s bridge, and
    /// tests make theirs here.
    pub fn new(line: impl Into<String>, reply: impl FnOnce(&str) + Send + 'static) -> Self {
        Self {
            line: line.into(),
            reply: Some(Box::new(reply)),
        }
    }

    /// Send the JSON answer back to the launcher.
    pub fn answer(mut self, json: &str) {
        if let Some(reply) = self.reply.take() {
            reply(json);
        }
    }
}

impl Drop for Request {
    /// A request nobody answered still answers, so the launcher never waits on a line the game dropped.
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            reply(r#"{"error":"the game dropped this request"}"#);
        }
    }
}

static QUEUE: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());

/// Bundled iOS builds expose accounts and game management before Play.
#[cfg(target_os = "ios")]
const ENABLED: bool = true;

/// The native launcher fully covers the game's surface. Its bridge still needs polling.
static COVERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_covered(covered: bool) {
    COVERED.store(covered, std::sync::atomic::Ordering::Relaxed);
}

pub fn covered() -> bool {
    COVERED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Queue a request for the game's next frame.
pub fn push(request: Request) {
    QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push_back(request);
}

/// The requests the launcher sent since the last call, oldest first. Empty everywhere but a phone.
pub fn take() -> Vec<Request> {
    QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
        .collect()
}

/// The result of canceling one owned launcher start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelDecision {
    /// The matching start must finish draining before its cancel is acknowledged.
    Accepted,
    /// Another start owns the engine, or this attempt has already been cleared.
    Stale,
    /// This start has already uncovered the game and cannot be canceled by an old task.
    Committed,
}

/// Logical ownership is kept beside the loader's start gate, so a delayed
/// cancellation cannot reset a newer attempt or let a new attempt bypass drain.
struct StartState {
    started: Option<bool>,
    owner: Option<String>,
    canceling: Option<String>,
    committed: bool,
}

impl StartState {
    const fn new() -> Self {
        Self {
            started: None,
            owner: None,
            canceling: None,
            committed: false,
        }
    }

    fn start_owned(&mut self, owner: Option<&str>) -> bool {
        if self.canceling.is_some() {
            return false;
        }
        if self.started != Some(true) {
            self.committed = false;
        }
        if let Some(owner) = owner {
            self.owner = Some(owner.to_owned());
        }
        // A legacy fallback must not erase an owned start's identity. Likewise,
        // starting an already visible engine does not make it cancelable again.
        self.started = Some(true);
        true
    }

    fn request_cancel(&mut self, owner: &str) -> CancelDecision {
        if self.owner.as_deref() != Some(owner) {
            return CancelDecision::Stale;
        }
        if self.committed {
            return CancelDecision::Committed;
        }
        self.canceling = self.owner.clone();
        // Keep the old loader's gate open until it has actually drained. Closing
        // it now could leave an awakened worker asleep again during shutdown.
        CancelDecision::Accepted
    }

    fn commit_start(&mut self) -> bool {
        if self.started != Some(true) || self.canceling.is_some() {
            return false;
        }
        self.committed = true;
        true
    }

    fn commit_start_owned(&mut self, owner: &str) -> bool {
        self.owner.as_deref() == Some(owner) && self.commit_start()
    }

    fn complete_cancel(&mut self, owner: &str, default_open: bool) -> bool {
        if self.owner.as_deref() != Some(owner) || self.canceling.as_deref() != Some(owner) {
            return false;
        }
        self.reset(default_open);
        true
    }

    fn reset(&mut self, default_open: bool) {
        *self = Self {
            started: Some(default_open),
            ..Self::new()
        };
    }
}

/// Whether the game may start loading: not until the player starts it from the launcher.
static STARTED: (Mutex<StartState>, std::sync::Condvar) =
    (Mutex::new(StartState::new()), std::sync::Condvar::new());

/// Wait for the player to start the game from the launcher; at once in a build or bundle without one.
/// True when it waited, so options changed meanwhile must be read again.
pub fn wait_for_start() -> bool {
    let (started, signal) = &STARTED;
    let mut state = started.lock().unwrap_or_else(PoisonError::into_inner);
    let open = *state.started.get_or_insert_with(|| !bundled());
    if open {
        return false;
    }
    while state.started != Some(true) {
        state = signal.wait(state).unwrap_or_else(PoisonError::into_inner);
    }
    true
}

/// Let the game load without an owned launcher attempt (legacy/fallback callers).
pub fn start() {
    let _ = start_owned(None);
}

/// Begin or acknowledge an owned attempt. Reject every start while a previous
/// cancellation is still draining. IDs are opaque strings supplied by the launcher.
pub fn start_owned(owner: Option<&str>) -> bool {
    let (started, signal) = &STARTED;
    let accepted = started
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .start_owned(owner);
    if accepted {
        signal.notify_all();
    }
    accepted
}

/// Request cancellation only of the current, still-covered start.
pub fn request_cancel(owner: &str) -> CancelDecision {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .request_cancel(owner)
}

pub fn cancellation_requested() -> bool {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .canceling
        .is_some()
}

/// The attempt whose physical engine/loader teardown must complete before acknowledgment.
pub fn cancellation_owner() -> Option<String> {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .canceling
        .clone()
}

/// Commit the current start when the launcher actually uncovers the game.
pub fn commit_start() -> bool {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .commit_start()
}

/// Commit only the current matching attempt before the launcher uncovers the
/// game. This is the atomic decision point between canceling and entering play.
pub fn commit_start_owned(owner: &str) -> bool {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .commit_start_owned(owner)
}

/// Complete only the matching cancellation, after its workers have drained.
/// A stale completion cannot reset a newer start.
pub fn complete_cancel(owner: &str) -> bool {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .complete_cancel(owner, !bundled())
}

/// Return to launcher-only state after the engine has shut down normally.
pub fn reset() {
    STARTED
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reset(!bundled());
}

/// Whether the game is still waiting to be started.
pub fn waiting() -> bool {
    let state = STARTED.0.lock().unwrap_or_else(PoisonError::into_inner);
    state.started.is_none_or(|open| !open) && bundled()
}

/// Whether this bundle carries the launcher, which holds the game until it is started.
fn bundled() -> bool {
    #[cfg(target_os = "ios")]
    return ENABLED && ios::framework().is_some();
    #[cfg(not(target_os = "ios"))]
    false
}

/// Load the launcher and show it over the game, once per run: the loading screen's window asks, and so does
/// the game's when it makes its own. False when this build or bundle has none, or it was already shown. A
/// launcher that can't be shown can't start the game either, so the game starts by itself then.
pub fn present() -> bool {
    static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let shown = show();
    if !shown {
        start();
    }
    shown
}

/// Show the launcher over the game again (Quit Game, which an iPhone app does not do itself). False when this
/// build or bundle has none; the launcher ignores a second request while it is up.
pub fn show() -> bool {
    #[cfg(target_os = "ios")]
    return ENABLED && ios::present();
    #[cfg(not(target_os = "ios"))]
    false
}

/// Show the game files download over the game, in SwiftUI (packaging/ios/Launcher/DownloadView.swift): upright,
/// with its progress, speed and time left, asked of the game with `download-state`. Shown whether or not the
/// launcher is switched on, since it only needs the framework. False when this build or bundle has none, and the
/// game's own download screen is all there is.
pub fn present_download() -> bool {
    #[cfg(target_os = "ios")]
    return ios::call(ios::DOWNLOAD);
    #[cfg(not(target_os = "ios"))]
    false
}

/// Snapshot ABI shared with TouchControls.swift: ten normalized button centres,
/// NaN for the built-in placement, and bit zero to hide paint only (never input).
/// An older/missing launcher keeps the original layout and visibility.
pub fn touch_preferences() -> ([f32; 20], bool) {
    let positions = [f32::NAN; 20];
    #[cfg(target_os = "ios")]
    let (positions, hidden) = {
        let mut positions = positions;
        let hidden = ios::touch_preferences(&mut positions);
        (positions, hidden)
    };
    #[cfg(not(target_os = "ios"))]
    let hidden = false;
    (positions, hidden)
}

#[cfg(target_os = "ios")]
mod ios {
    use std::ffi::{CStr, CString, c_char, c_void};

    /// `horizon_launcher_present` in packaging/ios/Launcher/Present.swift.
    const SYMBOL: &CStr = c"horizon_launcher_present";
    /// `horizon_download_present` in packaging/ios/Launcher/DownloadView.swift.
    pub const DOWNLOAD: &CStr = c"horizon_download_present";

    /// How the launcher hears back: its context, and the answer as NUL-terminated UTF-8 JSON that lives
    /// only for the call.
    type Done = extern "C" fn(ctx: *mut c_void, reply: *const c_char);
    /// How the launcher asks: a NUL-terminated line, and where its answer goes.
    type Submit = extern "C" fn(line: *const c_char, ctx: *mut c_void, done: Done);

    extern "C" fn submit(line: *const c_char, ctx: *mut c_void, done: Done) {
        let line = if line.is_null() {
            String::new()
        } else {
            // SAFETY: the launcher passes a NUL-terminated string it keeps alive for this call
            // (`withCString` in Bridge.swift), and nothing here keeps the pointer past it.
            unsafe { CStr::from_ptr(line) }
                .to_string_lossy()
                .into_owned()
        };
        // The context is the launcher's: carried back untouched, as an address so the reply is Send.
        let ctx = ctx as usize;
        super::push(super::Request::new(line, move |json: &str| {
            let text = CString::new(json.replace('\0', "")).unwrap_or_default();
            done(ctx as *mut c_void, text.as_ptr());
        }));
    }

    /// The launcher framework's binary in this bundle, if it has one.
    pub fn framework() -> Option<CString> {
        let framework = std::env::current_exe()
            .ok()?
            .parent()?
            .join("Frameworks/HorizonLauncherKit.framework/HorizonLauncherKit");
        framework
            .exists()
            .then(|| CString::new(framework.into_os_string().into_encoded_bytes()).ok())?
    }

    pub fn touch_preferences(positions: &mut [f32; 20]) -> bool {
        // Resolve once, retaining the framework handle for the process lifetime.
        // No disk reads or dlopen calls in subsequent input/render frames.
        type Snapshot = extern "C" fn(*mut f32, u32) -> u32;
        static SNAPSHOT: std::sync::OnceLock<Option<Snapshot>> = std::sync::OnceLock::new();
        let snapshot = SNAPSHOT.get_or_init(|| {
            let path = framework()?;
            // SAFETY: this bundled Swift C export accepts exactly 20 writable
            // floats and a capacity, keeps no pointer and uses a locked snapshot.
            unsafe {
                let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
                if handle.is_null() {
                    return None;
                }
                let symbol = libc::dlsym(handle, c"horizon_touch_preferences_v1".as_ptr());
                if symbol.is_null() {
                    return None;
                }
                Some(std::mem::transmute::<*mut c_void, Snapshot>(symbol))
            }
        });
        snapshot.is_some_and(|call| call(positions.as_mut_ptr(), 20) & 1 != 0)
    }

    pub fn present() -> bool {
        call(SYMBOL)
    }

    /// Call `symbol` in the launcher framework with the bridge; false when there is no framework or symbol.
    pub fn call(symbol_name: &CStr) -> bool {
        let Some(path) = framework() else {
            log::info!("Horizon: no launcher framework in this bundle");
            return false;
        };
        // SAFETY: dlopen and dlsym on a framework Xcode built and signed into this bundle; the symbol is a
        // `@_cdecl` Swift function taking the bridge's C function pointer and returning a Bool, called on
        // the main thread (the game's window event), which is where UIKit has to be touched.
        unsafe {
            let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                log::warn!("Horizon: the launcher framework did not load");
                return false;
            }
            let symbol = libc::dlsym(handle, symbol_name.as_ptr());
            if symbol.is_null() {
                log::warn!("Horizon: the launcher framework has no {symbol_name:?}");
                return false;
            }
            let present: extern "C" fn(Submit) -> bool = std::mem::transmute(symbol);
            present(submit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_desktop_never_waits_to_be_started() {
        assert!(!waiting());
        assert!(!wait_for_start());
    }

    #[test]
    fn owned_cancel_is_idempotent_and_blocks_new_starts_until_matching_completion() {
        let mut state = StartState::new();
        assert!(state.start_owned(Some("attempt A")));
        assert!(state.start_owned(Some("attempt A")));
        assert_eq!(state.request_cancel("attempt A"), CancelDecision::Accepted);
        assert_eq!(state.request_cancel("attempt A"), CancelDecision::Accepted);
        assert_eq!(state.canceling.as_deref(), Some("attempt A"));
        // Do not close the old worker's gate while shutdown is still draining.
        assert_eq!(state.started, Some(true));
        assert!(!state.start_owned(Some("attempt B")));
        assert!(!state.start_owned(None));
        assert!(!state.commit_start());
        assert!(!state.complete_cancel("attempt B", false));
        assert_eq!(state.canceling.as_deref(), Some("attempt A"));
        assert!(state.complete_cancel("attempt A", false));
        assert_eq!(state.started, Some(false));
        assert!(state.owner.is_none() && state.canceling.is_none());
        assert!(!state.complete_cancel("attempt A", false));
        assert!(state.start_owned(Some("attempt B")));
        assert_eq!(state.request_cancel("attempt A"), CancelDecision::Stale);
        assert!(!state.complete_cancel("attempt A", false));
        assert_eq!(state.owner.as_deref(), Some("attempt B"));
        assert_eq!(state.started, Some(true));
    }

    #[test]
    fn replacing_an_uncommitted_start_does_not_let_its_old_task_cancel_the_new_one() {
        let mut state = StartState::new();
        assert!(state.start_owned(Some("old")));
        assert!(state.start_owned(Some("new")));
        assert_eq!(state.request_cancel("old"), CancelDecision::Stale);
        assert!(!state.complete_cancel("old", false));
        assert!(state.canceling.is_none());
        assert_eq!(state.request_cancel("new"), CancelDecision::Accepted);
        assert!(state.complete_cancel("new", false));
    }

    #[test]
    fn owned_commit_requires_the_current_attempt_and_loses_to_an_accepted_cancel() {
        let mut state = StartState::new();
        assert!(!state.commit_start_owned("A"));
        assert!(state.start_owned(Some("A")));
        assert!(!state.commit_start_owned("wrong"));
        assert!(!state.committed);
        assert!(state.commit_start_owned("A"));
        assert!(state.commit_start_owned("A"));
        assert_eq!(state.request_cancel("A"), CancelDecision::Committed);
        assert!(state.start_owned(Some("B")));
        assert!(!state.commit_start_owned("A"));
        assert!(state.commit_start_owned("B"));
        assert_eq!(state.request_cancel("A"), CancelDecision::Stale);
        assert_eq!(state.request_cancel("B"), CancelDecision::Committed);

        state.reset(false);
        assert!(state.start_owned(Some("A")));
        assert_eq!(state.request_cancel("A"), CancelDecision::Accepted);
        assert!(!state.commit_start_owned("A"));
        assert!(!state.committed);
        assert_eq!(state.canceling.as_deref(), Some("A"));
        assert!(state.complete_cancel("A", false));
        assert!(state.start_owned(Some("B")));
        assert!(!state.commit_start_owned("A"));
        assert!(state.commit_start_owned("B"));
    }

    #[test]
    fn a_visible_engine_stays_committed_until_normal_reset() {
        let mut state = StartState::new();
        assert!(!state.commit_start());
        assert!(state.start_owned(Some("first")));
        assert!(state.commit_start());
        assert!(state.start_owned(Some("first")));
        assert_eq!(state.request_cancel("first"), CancelDecision::Committed);
        // A new task asking to use an already visible engine cannot make that
        // engine cancelable again, nor can the first task stop its replacement.
        assert!(state.start_owned(Some("second")));
        assert_eq!(state.request_cancel("first"), CancelDecision::Stale);
        assert_eq!(state.request_cancel("second"), CancelDecision::Committed);
        assert!(!state.complete_cancel("second", false));
        state.reset(false);
        assert!(state.owner.is_none() && state.canceling.is_none());
        assert!(!state.committed);
        assert!(state.start_owned(Some("third")));
        assert_eq!(state.request_cancel("second"), CancelDecision::Stale);
        assert_eq!(state.request_cancel("third"), CancelDecision::Accepted);
    }

    #[test]
    fn legacy_start_preserves_owned_identity_and_reset_keeps_the_platform_default() {
        let mut state = StartState::new();
        assert!(state.start_owned(None));
        assert_eq!(state.request_cancel("unknown"), CancelDecision::Stale);
        assert!(state.start_owned(Some("owned")));
        assert!(state.start_owned(None));
        assert_eq!(state.owner.as_deref(), Some("owned"));
        assert_eq!(state.request_cancel("owned"), CancelDecision::Accepted);
        assert!(state.complete_cancel("owned", true));
        assert_eq!(state.started, Some(true));
        assert!(state.owner.is_none() && state.canceling.is_none());
        assert!(!state.committed);
    }

    #[test]
    fn normal_reset_clears_pending_ownership_after_engine_shutdown() {
        let mut state = StartState::new();
        assert!(state.start_owned(Some("old")));
        assert_eq!(state.request_cancel("old"), CancelDecision::Accepted);
        state.reset(false);
        assert_eq!(state.started, Some(false));
        assert_eq!(state.request_cancel("old"), CancelDecision::Stale);
        assert!(!state.complete_cancel("old", false));
        assert!(state.start_owned(Some("new")));
        assert!(state.commit_start());
        assert_eq!(state.request_cancel("old"), CancelDecision::Stale);
    }

    #[test]
    fn requests_come_out_in_order_and_answer_once() {
        let answers = Arc::new(Mutex::new(Vec::new()));
        for line in ["worlds", "servers"] {
            let answers = answers.clone();
            push(Request::new(line, move |json: &str| {
                answers.lock().unwrap().push(json.to_owned())
            }));
        }
        let taken = take();
        assert_eq!(
            taken.iter().map(|r| r.line.as_str()).collect::<Vec<_>>(),
            ["worlds", "servers"]
        );
        assert!(take().is_empty());
        let mut taken = taken.into_iter();
        taken.next().unwrap().answer(r#"{"ok":true}"#);
        // Dropped unanswered: the launcher still hears back.
        drop(taken);
        let answers = answers.lock().unwrap();
        assert_eq!(answers[0], r#"{"ok":true}"#);
        assert!(answers[1].contains("error"));
        assert_eq!(answers.len(), 2);
    }
}
