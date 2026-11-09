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
    pub use horizon_hooks::{Hooks, Started, TooLate, register};
}

/// The multiplayer server list as vanilla stores it (`servers.dat`): read, write, add, remove,
/// and the lock held while it is read and written back.
pub mod servers {
    pub use horizon_protocol::servers::{
        ServerEntry, load, lock, parse_address, prepend, remove_where, save,
    };
}
