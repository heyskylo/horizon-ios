//! The platform's public API: what the Horizon layer, mods and ports may use of the engine
//! (docs/layers.md#platform). The Horizon layer depends on the platform only, never on an engine
//! crate, so it can build from another repository; when it needs something the engine has, it
//! comes through here.
//!
//! Today this re-exports engine modules whole. Each one gets its own stable types as the split
//! goes on (docs/layers.md#plan, M6).

/// The machine the game runs on (`horizon-system`): OS, CPU, memory, drive, power, GPU maker and
/// memory.
pub use horizon_system as system;

/// Plugging into the game (`horizon-hooks`): implement [`hooks::Hooks`] and
/// [`hooks::register`] it before the game starts (docs/layers.md, "How a Horizon crate plugs
/// in"). The engine's dispatch side is not part of the API.
pub mod hooks {
    pub use horizon_hooks::{Hooks, Started, TooLate, register, seat};
}

/// The settings store (`horizon_hooks::settings`): a crate's own `key:value` lines in the game
/// folder, loaded and saved with the player's settings (docs/layers.md, "Platform registries").
pub mod settings {
    pub use horizon_hooks::settings::{Store, register};
}

/// Settings pages a crate adds to the options screens (docs/layers.md, "Platform registries").
pub mod pages {
    pub use horizon_hooks::pages::{Act, Label, Page, Place, Row, register};
}

/// Drawing on the HUD (docs/layers.md, "Platform registries").
pub mod hud {
    pub use horizon_hooks::hud::{Canvas, Corner, Layer, register};
}
