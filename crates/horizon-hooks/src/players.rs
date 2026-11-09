//! The players the game shows, as a crate above the engine sees them (Horizon's emotes, spec
//! #14091): [`PlayerModels`] in [`crate::Hooks::tick`], [`Frame`] in [`crate::Hooks::frame`],
//! [`BodyPose`], a pose laid over a player's body after its vanilla animation, what goes by their
//! names, and the crate's [`Sprites`]. Plain types only: no engine type crosses this line.
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
    /// The player's UUID as the server gave it (the window's own: its profile); None for
    /// mannequins.
    pub uuid: Option<u128>,
    /// Its age in ticks, the clock a [`BodyPose`] is timed on.
    pub age: u32,
    /// It moved sideways since the last tick (more than 0.01 blocks).
    pub moved: bool,
    /// What it wears that covers a crate's drawing.
    pub gear: Gear,
    /// It is drawn invisible (the effect, or a trader's disguise).
    pub invisible: bool,
}

impl PlayerModel {
    pub fn new(id: u32, kind: Kind, uuid: Option<u128>, age: u32, moved: bool) -> Self {
        Self {
            id,
            kind,
            uuid,
            age,
            moved,
            gear: Gear::default(),
            invisible: false,
        }
    }

    /// With what it wears and whether it is invisible.
    pub fn looks(self, gear: Gear, invisible: bool) -> Self {
        Self {
            gear,
            invisible,
            ..self
        }
    }
}

/// What a player model wears where a crate may draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Gear {
    /// A helmet or a head item.
    pub head: bool,
    /// A chestplate (an elytra is on the back).
    pub chest: bool,
    /// A cape or an elytra.
    pub back: bool,
}

impl Gear {
    pub fn new(head: bool, chest: bool, back: bool) -> Self {
        Self { head, chest, back }
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
    /// The world is the window's own single-player one: its server names the player by an
    /// offline profile.
    pub solo: bool,
}

impl Frame {
    pub fn new(connection: u64, solo: bool) -> Self {
        Self { connection, solo }
    }
}

/// Rectangles drawn before a name: x, y, width, height in text pixels from the name line's top
/// left corner (a pixel left of its background), and `0xAARRGGBB`.
pub type Mark = &'static [(u8, u8, u8, u8, u32)];

/// What a crate draws by a player's name over their head ([`crate::Hooks::name_decoration`]).
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct NameDecoration {
    /// Before the name.
    pub mark: Option<Mark>,
    /// After the name, and a line under it.
    pub badge: Option<Badge>,
    /// How high the crate's drawing on the body gets above the neck, in model pixels: the tag
    /// goes up over it.
    pub top: Option<f32>,
}

impl NameDecoration {
    pub fn new(mark: Option<Mark>, badge: Option<Badge>, top: Option<f32>) -> Self {
        Self { mark, badge, top }
    }
}

/// A badge after a name: 8 text pixels square, a pixel right of the background.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Badge {
    /// Its picture: a part of a [`Sprites`] sprite, in atlas coordinates (u0, v0, u1, v1).
    pub uv: [f32; 4],
    /// The line under the name, `0xRRGGBB`.
    pub line: u32,
}

impl Badge {
    pub fn new(uv: [f32; 4], line: u32) -> Self {
        Self { uv, line }
    }
}

/// A crate's pictures in the game's atlas, by ids of its own: 64×64 RGBA each, in the slots skins
/// use (one no frame drew for a while may go). Read while drawing.
pub trait Sprites {
    /// Where sprite `id` is (u0, v0, u1, v1), and it is kept this frame. None until
    /// [`SpriteStore::put`] put it there.
    fn get(&self, id: u64) -> Option<[f32; 4]>;
}

/// Where a crate puts its [`Sprites`], in [`crate::Hooks::frame`].
pub trait SpriteStore {
    /// Sprite `id` is in the atlas.
    fn has(&self, id: u64) -> bool;
    /// Puts sprite `id` in the atlas (64×64 RGBA), if a slot is free.
    fn put(&mut self, id: u64, rgba: Vec<u8>);
}

/// The window's own model, as [`crate::Hooks::camera`] sees it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CameraView {
    /// Its age in ticks, with the frame's part of a tick.
    pub age: f32,
}

impl CameraView {
    pub fn new(age: f32) -> Self {
        Self { age }
    }
}

/// How a crate moves the camera in third person.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CameraOverride {
    /// How far it stands back, as a share of the usual distance.
    pub distance: f32,
}

impl CameraOverride {
    pub fn new(distance: f32) -> Self {
        Self { distance }
    }
}

/// A model a crate draws on players, as the game's own models are: a tree of parts, 16 units a
/// block, y down. Build it once and keep the `Arc`: the engine prepares it for the GPU once, by
/// the `Arc`'s address, while the crate holds it.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Mesh {
    /// What [`PartPoses::pose`] names it by.
    pub name: String,
    /// At rest: x, y, z, x/y/z rotation (radians), x/y/z scale.
    pub pose: [f32; 9],
    pub quads: Vec<Quad>,
    pub children: Vec<Mesh>,
}

impl Mesh {
    pub fn new(name: String, pose: [f32; 9], quads: Vec<Quad>, children: Vec<Mesh>) -> Self {
        Self {
            name,
            pose,
            quads,
            children,
        }
    }
}

/// One face of a [`Mesh`].
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Quad {
    pub normal: [f32; 3],
    /// Its corners: x, y, z, then u, v across the sprite (0 to 1).
    pub vertices: [[f32; 5]; 4],
}

impl Quad {
    pub fn new(normal: [f32; 3], vertices: [[f32; 5]; 4]) -> Self {
        Self { normal, vertices }
    }
}

/// A part of the player's body a crate draws on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bone {
    Head,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

impl Bone {
    /// Its name among [`BODY_PARTS`].
    pub fn part(self) -> &'static str {
        BODY_PARTS[self as usize]
    }
}

/// Which picture of a player is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pass {
    /// In the world, seen by the camera.
    World,
}

/// The poses of a [`Mesh`]'s parts for one [`PlayerDraw::draw`], each at rest to begin with.
pub trait PartPoses {
    /// Part `name`'s pose, to change (see [`Mesh::pose`]); a name the mesh lacks gets a pose that
    /// is not used.
    fn pose(&mut self, name: &str) -> &mut [f32; 9];
}

/// Where a crate draws on a player model ([`crate::Hooks::player_layers`]). Matrices are
/// column-major, relative to the camera, in blocks.
pub trait PlayerDraw {
    fn pass(&self) -> Pass;
    /// The model's age in ticks, with the frame's part of a tick.
    fn ticks(&self) -> f32;
    /// Where `bone` is as the body is drawn this frame; None if the model has no such part.
    fn bone(&mut self, bone: Bone) -> Option<[[f32; 4]; 4]>;
    /// The pose laid over the model's body ([`PlayerModels::set_body_pose`]), if any.
    fn body_pose(&self) -> Option<&Arc<dyn BodyPose>>;
    /// As [`Sprites::get`].
    fn sprite(&self, id: u64) -> Option<[f32; 4]>;
    /// Draws `mesh` at `matrix`, textured by the sprite part `uv` (u0, v0, u1, v1) and lit as the
    /// model is; `pose` poses its parts first.
    fn draw(
        &mut self,
        mesh: &Arc<Mesh>,
        matrix: [[f32; 4]; 4],
        uv: [f32; 4],
        pose: &mut dyn FnMut(&mut dyn PartPoses),
    );
}
