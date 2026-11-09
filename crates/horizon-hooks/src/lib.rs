//! The game's extension points for code above the engine (docs/layers.md, "How a Horizon crate
//! plugs in"). A platform or Horizon crate implements [`Hooks`] and [`register`]s it before the
//! game starts; the engine calls every registered hook at the points below. This crate names no
//! type from above, so the engine can offer it; `horizon-platform` re-exports it as
//! `horizon_platform::hooks`, which is where the Horizon layer uses it from.
//!
//! Registration happens once, before the game runs: the list is sealed the first time the engine
//! dispatches, and a hook registered after that is refused. With nothing registered a dispatch
//! is one load and an empty loop.
//!
//! A new hook is a new method with an empty default body, so existing implementations keep
//! compiling, and a dispatch function in [`engine`] that the engine calls at the place vanilla
//! does the matching thing.
//!
//! The platform registries (docs/layers.md, "Platform registries") have the same shape:
//! [`settings`] for the settings each crate keeps in its own lines, [`pages`] for its settings
//! pages, [`hud`] for what it draws on the HUD.
#![forbid(unsafe_code)]

pub mod hud;
pub mod pages;
pub mod players;
mod registry;
pub mod settings;

use registry::Registry;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a crate above the engine hears from the game. Every method has an empty default:
/// implement only those you need. Hooks run on the game's thread (the render thread) unless a
/// method says otherwise; they must return quickly and must not panic.
pub trait Hooks: Sync {
    /// The game started for someone playing it: once, after the log and the crash handler, before
    /// the first frame. Not for a dedicated server, a benchmark, an offscreen run or a run that
    /// quits by itself or that horizon-ctl drives (those start in a source checkout).
    fn started(&self, _game: &Started<'_>) {}

    /// The world this window plays was opened to LAN or to friends (`true`), or no longer is
    /// (`false`). Called when it changes, not every frame.
    fn published(&self, _open: bool) {}

    /// The game is ending normally: its window closed and the world is saved. Not after a crash.
    fn shutdown(&self) {}

    /// Once a frame, before drawing. Only a window's game calls it (each splitscreen player in
    /// turn, [`seat`]), never the integrated server's thread or a dedicated server.
    fn frame(&self, _frame: &players::Frame) {}

    /// Once a client tick while a world is shown: the player models in view. Called as
    /// [`Hooks::frame`] is.
    fn tick(&self, _models: &mut dyn players::PlayerModels) {}
}

/// What [`Hooks::started`] gets.
#[non_exhaustive]
pub struct Started<'a> {
    /// The build: the release tag (`0.2.31`), or `dev <crate version>` for a build from source.
    pub version: &'a str,
}

impl<'a> Started<'a> {
    pub fn new(version: &'a str) -> Self {
        Self { version }
    }
}

static HOOKS: Registry<dyn Hooks> = Registry::new();

/// Why [`register`] refused a hook.
#[derive(Debug, PartialEq, Eq)]
pub struct TooLate;

impl std::fmt::Display for TooLate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the game already started: hooks register before it runs")
    }
}

impl std::error::Error for TooLate {}

/// Adds `hooks` to those the game calls. Call it before the game starts (from the executable's
/// `main`, before it runs the game); once the engine has dispatched, the list is fixed and this
/// returns [`TooLate`]. Hooks are called in the order they registered.
pub fn register(hooks: &'static dyn Hooks) -> Result<(), TooLate> {
    HOOKS.register(hooks)
}

/// The registered hooks, sealing the list on the first call.
fn all() -> &'static [&'static dyn Hooks] {
    HOOKS.all()
}

thread_local! {
    static SEAT: Cell<usize> = const { Cell::new(0) };
}

/// The splitscreen player whose turn it is: 0 for P1 (and without splitscreen), 1 for P2...
/// Splitscreen players are one process, each `Game` in turn on the game's thread, so a crate's
/// statics are shared by them: a crate that keeps per-player state (settings, pages, HUD) indexes
/// it by this. A player keeps its seat while it plays. Other threads always see 0.
pub fn seat() -> usize {
    SEAT.get()
}

/// The engine's side: where it calls the hooks. Only the engine calls these.
pub mod engine {
    use super::*;

    /// The player whose turn starts on this thread ([`seat`]).
    pub fn set_seat(seat: usize) {
        SEAT.set(seat);
    }

    /// Seals every list (hooks and the registries); their first use does it anyway. Call where
    /// registration ends.
    pub fn seal() {
        all();
        crate::settings::all();
        crate::pages::engine::all();
        crate::hud::engine::all();
    }

    /// [`Hooks::started`].
    pub fn started(game: &Started<'_>) {
        for hooks in all() {
            hooks.started(game);
        }
    }

    /// Whether the world was open to LAN or friends at the last call.
    static PUBLISHED: AtomicBool = AtomicBool::new(false);

    /// [`Hooks::published`], when `open` differs from the last call's. Cheap enough to call
    /// every frame.
    pub fn published(open: bool) {
        let hooks = all();
        if !hooks.is_empty() && PUBLISHED.swap(open, Ordering::Relaxed) != open {
            for h in hooks {
                h.published(open);
            }
        }
    }

    /// [`Hooks::shutdown`].
    pub fn shutdown() {
        for hooks in all() {
            hooks.shutdown();
        }
    }

    /// Whether any hook is registered: without one the engine builds no view to hand them.
    pub fn any() -> bool {
        !all().is_empty()
    }

    /// [`Hooks::frame`].
    pub fn frame(frame: &players::Frame) {
        for hooks in all() {
            hooks.frame(frame);
        }
    }

    /// [`Hooks::tick`].
    pub fn tick(models: &mut dyn players::PlayerModels) {
        for hooks in all() {
            hooks.tick(models);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use players::PlayerModels as _;
    use std::sync::Mutex as StdMutex;

    /// Records what it heard. One process-wide list, as the game has: this test is the only
    /// one that registers or dispatches.
    struct Recorder(StdMutex<Vec<String>>);

    impl Hooks for Recorder {
        fn started(&self, game: &Started<'_>) {
            self.0
                .lock()
                .unwrap()
                .push(format!("started {}", game.version));
        }
        fn published(&self, open: bool) {
            self.0.lock().unwrap().push(format!("published {open}"));
        }
        fn frame(&self, frame: &players::Frame) {
            self.0
                .lock()
                .unwrap()
                .push(format!("frame {}", frame.connection));
        }
        fn tick(&self, models: &mut dyn players::PlayerModels) {
            let mut ids = Vec::new();
            models.for_each(&mut |m| ids.push(m.id));
            for id in ids {
                models.set_body_pose(id, Some(std::sync::Arc::new(Still)));
            }
        }
    }

    #[derive(Debug)]
    struct Still;
    impl players::BodyPose for Still {
        fn pose(&self, _: f32, _: &[[f32; 9]; 6], _: &mut [[f32; 9]; 6]) -> bool {
            false
        }
    }

    /// Two models, poses by id.
    struct Two(std::collections::BTreeMap<u32, std::sync::Arc<dyn players::BodyPose>>);
    impl players::PlayerModels for Two {
        fn connection(&self) -> u64 {
            1
        }
        fn for_each(&self, f: &mut dyn FnMut(players::PlayerModel)) {
            for id in [3, 7] {
                f(players::PlayerModel::new(
                    id,
                    players::Kind::Player,
                    None,
                    0,
                    false,
                ));
            }
        }
        fn body_pose(&self, id: u32) -> Option<&std::sync::Arc<dyn players::BodyPose>> {
            self.0.get(&id)
        }
        fn set_body_pose(&mut self, id: u32, pose: Option<std::sync::Arc<dyn players::BodyPose>>) {
            match pose {
                Some(p) => self.0.insert(id, p),
                None => self.0.remove(&id),
            };
        }
    }

    /// Implements nothing: the defaults do nothing.
    struct Silent;
    impl Hooks for Silent {}

    static RECORDER: Recorder = Recorder(StdMutex::new(Vec::new()));

    #[test]
    fn registered_hooks_hear_the_game_and_late_ones_are_refused() {
        register(&RECORDER).unwrap();
        register(&Silent).unwrap();
        engine::started(&Started::new("0.2.31"));
        // Changes only: the same state every frame is said once.
        for open in [false, true, true, true, false, false] {
            engine::published(open);
        }
        engine::shutdown();
        assert!(engine::any());
        engine::frame(&players::Frame::new(4));
        let mut models = Two(Default::default());
        engine::tick(&mut models);
        assert!(models.body_pose(3).is_some() && models.body_pose(7).is_some());
        assert_eq!(
            *RECORDER.0.lock().unwrap(),
            [
                "started 0.2.31",
                "published true",
                "published false",
                "frame 4"
            ]
        );
        assert_eq!(register(&Silent), Err(TooLate));
    }
}
