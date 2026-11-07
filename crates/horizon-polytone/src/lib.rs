//! Polytone-format resource pack files (`assets/<namespace>/polytone/...`), the format of the
//! Polytone mod's packs (#11486).
//!
//! Clean room: written from the public wiki (<https://github.com/MehVahdJukaar/polytone/wiki>) and
//! from observing packs, never from the mod's GPL source. This crate only reads the files: no GPU,
//! no gameplay. The game applies what it reads; nothing changes without a Polytone pack.
//!
//! Read so far: [`Targets`] (what a modifier aims at), and the [`BiomeModifier`] and
//! [`DimensionModifier`] environment attribute layers. Expressions stay source text
//! ([`Argument::Expression`]) until the expression language lands.
mod attributes;
mod modifiers;
mod target;

pub use attributes::{Argument, Attributes, Layer, Modifier, normalize_attribute};
pub use modifiers::{BiomeModifier, DimensionModifier};
pub use target::{Target, Targets, implicit_id};
