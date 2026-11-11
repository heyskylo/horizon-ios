//! Cosmetic items that Horizon clients draw on players: hats, back items,
//! shoulder pets, name tag styles, emotes, companions, trails and auras. Only Horizon clients
//! see them; the game protocol carries nothing about them
//! ([docs/cosmetics.md](../../../docs/cosmetics.md)).
//!
//! An item is a small model of boxes (Minecraft's box UV, as Blockbench's
//! "Modded Entity" format), one texture of at most 64×64, the bone of the
//! player model it rides on and a few simple looping animations; a name tag
//! style is a small mark and a line colour, with no model. This crate
//! reads and checks items and poses them; the game draws them.

pub mod account;
mod emote;
mod feed;
mod fetch;
pub mod follow;
mod fx;
mod item;
pub mod mark;
mod pose;
pub mod prefs;
pub mod preview;
pub mod trail;
pub mod transport;
pub mod viewer;
pub mod workers;

pub use emote::{
    BODY, Ease, Emote, Key, MAX_ALL_KEYS, MAX_KEYS, MAX_LENGTH, MAX_MOVE, MAX_ROOT_MOVE,
    MAX_ROOT_TURN, MAX_TRACKS, MAX_TURN, Playing, Sample, Target, Track,
};
pub use feed::{Feed, POLL, Seen};
pub use fetch::{BATCH, Clock, Cosmetics, Frame, Http, Local, Transport, Worn};
pub use fx::{
    Aura, Companion, From, HEAD, HEAD_CLEARANCE, MAX_AURA_RADIUS, MAX_COMPANION_ANCHOR,
    MAX_COMPANION_REACH, MAX_LAG, MAX_PARTICLE_SIZE, MAX_SPRITE, MAX_TRAIL_LIFETIME,
    MAX_TRAIL_RATE, MAX_TRAIL_SPRITES, Trail,
};
pub use item::{
    Animation, Axis, Bone, Error, FORMAT, Face, Item, MAX_MARK, MAX_PARTS, MAX_REACH, MAX_TEXTURE,
    MAX_VERTICES, Motion, Part, Slot, Tag, Texture,
};
pub use pose::{Pose, pose_matrix};

/// Cosmetics run by default (Ludovic, 2026-11-04: on when the shop opens): the settings entry and
/// your own are there for everyone. Other players' show by default only to an account that owns a
/// cosmetic (`prefs::Others::effective`, the same day: a player who owns none sees a vanilla game
/// and the game asks nothing about other players, as CONTRIBUTING.md asks), until the player
/// chooses. `HORIZON_COSMETICS=0` turns everything off.
pub const ON_BY_DEFAULT: bool = true;

/// The cosmetics service (`HORIZON_COSMETICS_API` overrides it).
pub const API: &str = horizon_addresses::COSMETICS_API;

/// Whether cosmetics run: `ON_BY_DEFAULT`, or `HORIZON_COSMETICS=1` / `0` for one run
/// (development, the bench).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| match std::env::var("HORIZON_COSMETICS").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => ON_BY_DEFAULT,
    })
}

/// `HORIZON_COSMETICS=1` asked for this run: the launcher shows its Wardrobe even while the
/// cosmetics service has no Shop to show (development, screenshots).
pub fn forced() -> bool {
    std::env::var("HORIZON_COSMETICS").as_deref() == Ok("1")
}
