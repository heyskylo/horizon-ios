//! The hook a dedicated server's chat filter fills: the hub asks a [`ChatGuard`] about each text a player
//! sends before it relays it, and carries out what it decides. The engine has no filter of its own; the
//! platform's `horizon-chat-filter` ([docs/chat-filter.md]) installs one ([`install`]) and the dedicated
//! server loads it from `server.toml` ([`load`]). Without one the hub relays every text, as vanilla's does.
use std::path::Path;
use std::sync::OnceLock;

pub use crate::duration::{
    DurationUnit, duration_parts, duration_text, now_millis, parse_duration,
};

/// Where a player's text was going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A chat message.
    Chat,
    /// A command's message: `/msg`, `/tell`, `/w`, `/me`, `/say`, `/teammsg`.
    Command,
    Sign,
    Anvil,
    Book,
}

impl Place {
    pub fn name(self) -> &'static str {
        match self {
            Place::Chat => "chat",
            Place::Command => "command",
            Place::Sign => "sign",
            Place::Anvil => "anvil",
            Place::Book => "book",
        }
    }

    /// Chat and commands: a muted player can't use them.
    pub fn is_chat(self) -> bool {
        matches!(self, Place::Chat | Place::Command)
    }
}

/// What a hit leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The text is refused, nothing more.
    Refused,
    /// Refused, and the player can't chat until then (epoch milliseconds).
    Muted { until: i64 },
    /// Refused, and the player is banned until then.
    Banned { until: i64 },
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::Refused => "refused",
            Action::Muted { .. } => "muted",
            Action::Banned { .. } => "banned",
        }
    }
}

/// A player's mute.
#[derive(Clone, Debug, PartialEq)]
pub struct Mute {
    pub uuid: String,
    pub name: String,
    /// Epoch milliseconds; `None`: until `/unmute`.
    pub until: Option<i64>,
    /// The operator who muted them (`/mute`, `/tempmute`); `None`: the filter.
    pub by: Option<String>,
    pub reason: Option<String>,
}

impl Mute {
    pub fn active(&self, now: i64) -> bool {
        self.until.is_none_or(|until| until > now)
    }
}

/// A server's chat filter: its word list, the players' hits and the mutes.
pub trait ChatGuard: Send + Sync {
    /// Whether `text` holds a word of the list.
    fn matches(&self, text: &str) -> bool;
    /// The player's mute, if they are muted at `now`.
    fn muted(&self, uuid: u128, now: i64) -> Option<&Mute>;
    /// The mutes in force at `now`, by name.
    fn mutes(&self, now: i64) -> Vec<&Mute>;
    /// An operator's `/mute` (`until` `None`) or `/tempmute`: replaces the player's mute.
    fn mute(
        &mut self,
        uuid: u128,
        name: &str,
        until: Option<i64>,
        by: &str,
        reason: Option<String>,
    );
    /// A hit by the player at `now`: the hit's number, and what it leads to.
    fn strike(&mut self, uuid: u128, name: &str, now: i64) -> (u32, Action);
    /// Lifts a player's mute, by name; the name as muted, if they were.
    fn unmute(&mut self, name: &str) -> Option<String>;
    /// Writes a refused text to the filter's own log, when it keeps one; `Err` says why it couldn't.
    #[allow(clippy::too_many_arguments)]
    fn log(
        &mut self,
        now: i64,
        name: &str,
        uuid: u128,
        place: Place,
        count: u32,
        action: Action,
        text: &str,
    ) -> Result<(), String>;
}

/// What [`load`] gives: the filter (`None`: off) and the lines to log, as (level, line).
#[derive(Default)]
pub struct Loaded {
    pub guard: Option<Box<dyn ChatGuard>>,
    pub log: Vec<(&'static str, String)>,
}

/// Reads a filter's settings from `server.toml`'s text, its files from the server's folder `dir`.
pub type Loader = fn(server_toml: &str, dir: &Path) -> Loaded;

static LOADER: OnceLock<Loader> = OnceLock::new();

/// Gives the dedicated server its chat filter, once, before it starts; a second call is ignored.
pub fn install(loader: Loader) {
    let _ = LOADER.set(loader);
}

/// The installed filter for this `server.toml`; none without one.
pub fn load(server_toml: &str, dir: &Path) -> Loaded {
    LOADER
        .get()
        .map_or_else(Loaded::default, |load| load(server_toml, dir))
}
