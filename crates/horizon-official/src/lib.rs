//! The official game: the platform's game (`horizon-game`) with Horizon's own in-game parts
//! plugged in through the platform's hooks (docs/layers.md, "How a Horizon crate plugs in").
//! horizon-client keeps its own plain executable; this one is what players get once the release
//! builds ship it (layer plan M5).
//!
//! Each Horizon crate that runs in the game gives a `&'static` value implementing
//! `horizon_platform::hooks::Hooks`, and [`register`] adds it. Nothing here depends on an engine
//! crate.
#![forbid(unsafe_code)]

/// Registers Horizon's in-game parts with the game. The executable calls it before it runs the
/// game; the launcher's generated executable for native mods will too.
pub fn register() {
    // Each part: `horizon_platform::hooks::register(&its_crate::HOOKS)`. The "I'm playing" ping
    // comes here once this executable is the one players get.
}
