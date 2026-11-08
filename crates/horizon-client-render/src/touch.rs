//! On-screen touch controls for touch-screen devices (Android first). Nothing
//! here runs unless a touch surface is in use.
pub mod layout;
pub use layout::{Control, Mode, Placement, Settings};
