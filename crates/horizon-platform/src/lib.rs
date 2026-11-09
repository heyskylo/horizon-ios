//! The platform's public API: what the Horizon layer, mods and ports may use of the engine
//! (docs/layers.md#platform). The Horizon layer depends on the platform only, never on an engine
//! crate, so it can build from another repository; when it needs something the engine has, it
//! comes through here.
//!
//! Today this re-exports engine modules whole. Each one gets its own stable types as the split
//! goes on (docs/layers.md#plan, M6). The parts that pull in large engine crates sit behind
//! features (`world`, `protocol`, `prepare`, `server-files`), so a crate builds only what it uses.

/// The machine the game runs on (`horizon-system`): OS, CPU, memory, drive, power, GPU maker and
/// memory.
pub use horizon_system as system;

/// The pinned Minecraft version: its id, protocol and data version.
pub use horizon_core::version;

/// The player's "no AI" switch, which the launcher, the game and their agent tools share.
pub use horizon_core::no_ai;

/// The prepared folder's file of files (`prepared_files::FILE_NAME`).
pub use horizon_core::prepared_files;

/// Vanilla's world files, as tools outside the game read and write them.
#[cfg(feature = "world")]
pub mod world {
    pub use horizon_core::position::ChunkPos;
    pub use horizon_world::{compression, nbt, region};
}

/// What a server list and a profile head need of the network protocol.
#[cfg(feature = "protocol")]
pub mod protocol {
    pub use horizon_protocol::crypto::offline_uuid;
    pub use horizon_protocol::server_redirect;
}

/// The preparer: the game's files from the player's own Minecraft install.
#[cfg(feature = "prepare")]
pub use horizon_prepare as prepare;

/// A server's own files: `server.properties`, the op, white and ban lists, the user cache.
#[cfg(feature = "server-files")]
pub use horizon_server_files as server_files;

/// Plugging into the game (`horizon-hooks`): implement [`hooks::Hooks`] and
/// [`hooks::register`] it before the game starts (docs/layers.md, "How a Horizon crate plugs
/// in"). The engine's dispatch side is not part of the API.
pub mod hooks {
    pub use horizon_hooks::{Hooks, Started, TooLate, register};
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
