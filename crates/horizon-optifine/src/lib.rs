//! The files OptiFine-format resource packs share, below every feature that reads them (connected
//! textures, custom item textures, random and emissive entities, custom entity models, sky,
//! colours): `.properties` files, the matchers their values hold, their texture paths, and the
//! `optifine/` (or legacy `mcpatcher/`) folders that hold them. Each feature's own rules sit in
//! a module of their own, free of the game's types (`random`: random entity textures' rules;
//! `random_textures`: where a pack keeps their variants; [`ctm`]: connected textures; `cit`:
//! custom item textures, which the client matches against its stacks).
//!
//! Written from OptiFine's public documentation of the file formats (and ETF's, for its additions
//! to random entities); `cit` also from CIT Resewn's wiki and the conditions of its MIT-licensed
//! source (THIRD_PARTY_NOTICES.md). No OptiFine code was read.

pub mod cem;
pub mod cit;
pub mod ctm;
pub mod matchers;
pub mod paths;
pub mod properties;
pub mod random;
pub mod random_textures;

pub use matchers::{BlockMatch, IdList, Name, RangeList};
pub use paths::{properties_files, properties_files_in, resolve, valid};
pub use properties::Properties;
