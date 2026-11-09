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
