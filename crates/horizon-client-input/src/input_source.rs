//! The engine's side of the input source (docs/layers.md, "Controllers, splitscreen, replays and
//! UI pages"): what gameplay tells a device beyond the keyboard and mouse. Gameplay names these
//! types, never a controller's; the controller code answers.

/// Something that happened to the player that a device may let them feel (a controller's
/// vibration). The device decides how, or whether.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Feedback {
    /// The player broke a block.
    BlockBroken,
    /// An explosion this far away (blocks) with this power.
    Explosion { distance: f64, radius: f32 },
    /// A totem of undying saved the player.
    Totem,
}
