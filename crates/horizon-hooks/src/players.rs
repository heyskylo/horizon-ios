//! The players the game shows, as a crate above the engine sees them (Horizon's emotes, spec
//! #14091): [`PlayerModels`] in [`crate::Hooks::tick`], [`Frame`] in [`crate::Hooks::frame`], and
//! [`BodyPose`], a pose laid over a player's body after its vanilla animation. Plain types only:
//! no engine type crosses this line.
use std::sync::Arc;

/// The player model's parts a [`BodyPose`] poses, in its order.
pub const BODY_PARTS: [&str; 6] = [
    "head",
    "body",
    "right_arm",
    "left_arm",
    "right_leg",
    "left_leg",
];

/// A pose a crate lays over a player model's body after its vanilla animation (Horizon's emotes,
/// docs/cosmetics.md), handed to the model with [`PlayerModels::set_body_pose`]. The crate
/// downcasts its own back through [`std::any::Any`].
pub trait BodyPose: std::any::Any + std::fmt::Debug + Send + Sync {
    /// Poses the [`BODY_PARTS`] at `age` (the model's ticks): `at_rest` holds each part before the
    /// animation, `poses` as the animation left it (x, y, z, x/y/z rotation, x/y/z scale); a part
    /// the model lacks holds the identity in both and is not written back. False when it poses
    /// nothing at `age` (`poses` is then ignored).
    fn pose(&self, age: f32, at_rest: &[[f32; 9]; 6], poses: &mut [[f32; 9]; 6]) -> bool;
}

/// What a player model is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// The player of this window, seen from outside (third person, the inventory picture).
    Own,
    /// Another player.
    Player,
    /// A mannequin: a player model with no player.
    Mannequin,
}

/// One player model, as [`PlayerModels::for_each`] shows it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct PlayerModel {
    /// Its entity id on this connection ([`PlayerModels::connection`]).
    pub id: u32,
    pub kind: Kind,
    /// The player's UUID as the server gave it; None for the window's own model and mannequins.
    pub uuid: Option<u128>,
    /// Its age in ticks, the clock a [`BodyPose`] is timed on.
    pub age: u32,
    /// It moved sideways since the last tick (more than 0.01 blocks).
    pub moved: bool,
}

impl PlayerModel {
    pub fn new(id: u32, kind: Kind, uuid: Option<u128>, age: u32, moved: bool) -> Self {
        Self {
            id,
            kind,
            uuid,
            age,
            moved,
        }
    }
}

/// The player models of the world this window shows, once a tick ([`crate::Hooks::tick`]).
pub trait PlayerModels {
    /// Changes on every join: entity ids are per connection, so what a crate keeps by id goes
    /// when this changes.
    fn connection(&self) -> u64;
    /// Each player model in view (other players and mannequins; the window's own model first).
    fn for_each(&self, f: &mut dyn FnMut(PlayerModel));
    /// The pose laid over model `id`, if any.
    fn body_pose(&self, id: u32) -> Option<&Arc<dyn BodyPose>>;
    /// Lays `pose` over model `id` (None: its vanilla animation alone). Unknown ids are ignored.
    fn set_body_pose(&mut self, id: u32, pose: Option<Arc<dyn BodyPose>>);
}

/// What [`crate::Hooks::frame`] gets, once a frame before drawing.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Frame {
    /// As [`PlayerModels::connection`]; 0 with no world.
    pub connection: u64,
}

impl Frame {
    pub fn new(connection: u64) -> Self {
        Self { connection }
    }
}
