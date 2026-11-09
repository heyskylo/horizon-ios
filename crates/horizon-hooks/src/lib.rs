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
//! [`settings`] for the settings each crate keeps in its own lines.
#![forbid(unsafe_code)]

mod registry;
pub mod settings;

use registry::Registry;
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

    /// A game window opened: once, before the first frame, for every run that has a window,
    /// automated ones included ([`Client::automated`]); never for a dedicated server.
    fn client_started(&self, _client: &Client) {}

    /// What the player is doing, for a status shown outside the game (Discord's): called when it
    /// changes, not every frame. Splitscreen: the first player's only.
    fn activity(&self, _now: Activity) {}
}

/// What [`Hooks::client_started`] gets.
#[non_exhaustive]
pub struct Client {
    /// A run nobody plays: offscreen, a benchmark, a run that quits by itself after some frames.
    pub automated: bool,
}

impl Client {
    pub fn new(automated: bool) -> Self {
        Self { automated }
    }
}

/// What the player is doing ([`Hooks::activity`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Activity {
    /// The player keeps it to themselves: the Discord switch in Horizon Settings is off. (The
    /// switch moves to the Discord crate's own settings with the settings store.)
    Hidden,
    /// On the title screen or another menu outside a world.
    Menus,
    World {
        play: Play,
        dimension: Dimension,
        mode: GameMode,
    },
}

/// How the player is in a world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    Singleplayer,
    /// A server or someone else's LAN world.
    Multiplayer,
    /// Their world is open to LAN or friends; `players` counts the host, `max` is how many may
    /// join.
    Hosting {
        players: usize,
        max: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
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

/// The engine's side: where it calls the hooks. Only the engine calls these.
pub mod engine {
    use super::*;

    /// Seals every list (hooks and the registries); their first use does it anyway. Call where
    /// registration ends.
    pub fn seal() {
        all();
        crate::settings::all();
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

    /// [`Hooks::client_started`].
    pub fn client_started(client: &Client) {
        for hooks in all() {
            hooks.client_started(client);
        }
    }

    /// The activity at the last call (`None` before the first).
    static ACTIVITY: Mutex<Option<Activity>> = Mutex::new(None);

    /// [`Hooks::activity`], when `now` differs from the last call's. Cheap enough to call every
    /// frame: with no hook registered it only loads the empty list.
    pub fn activity(now: Activity) {
        let hooks = all();
        if hooks.is_empty() {
            return;
        }
        let mut last = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
        if last.replace(now) != Some(now) {
            drop(last);
            for h in hooks {
                h.activity(now);
            }
        }
    }

    /// [`Hooks::shutdown`].
    pub fn shutdown() {
        for hooks in all() {
            hooks.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        fn activity(&self, now: Activity) {
            self.0.lock().unwrap().push(format!("{now:?}"));
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
        for now in [
            Activity::Menus,
            Activity::Menus,
            Activity::Hidden,
            Activity::Hidden,
        ] {
            engine::activity(now);
        }
        engine::shutdown();
        assert_eq!(
            *RECORDER.0.lock().unwrap(),
            [
                "started 0.2.31",
                "published true",
                "published false",
                "Menus",
                "Hidden"
            ]
        );
        assert_eq!(register(&Silent), Err(TooLate));
    }
}
