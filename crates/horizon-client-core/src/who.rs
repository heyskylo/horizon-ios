//! Who an action came from or is aimed at: the player or a mob by id.

/// Who caused damage or is being targeted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Who {
    Player,
    Mob(u32),
}
