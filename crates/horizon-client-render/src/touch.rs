//! On-screen touch controls for touch-screen devices (Android first). Nothing
//! here runs unless a touch surface is in use.
pub mod aim;
pub mod layout;
pub use aim::{
    attack_indicator, camera_delta, hotbar_slot, item_places_entity_on_tap, item_uses_on_tap,
    projectile_crosshair, screen_direction,
};
pub use layout::{Control, Mode, Placement, Settings};
