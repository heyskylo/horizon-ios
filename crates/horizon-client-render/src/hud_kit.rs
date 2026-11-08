//! The HUD kit: small immediate-mode pieces Horizon's own overlays share, so they fit around
//! vanilla's HUD and look alike (`docs/ui-kit.md`). Nothing here reaches gameplay; the game
//! builds the inputs (`Game::hud_room`) and the overlays draw with them.
//!
//! - `room`: what of a view an overlay may use, clear of the boss bars.
//! - `plate`: the box behind a notice's text.
//!
//! Vanilla's own display lists (`hud_layout`, `screen_draw`) never use the kit: they stay
//! exactly vanilla's.
pub mod plate;
pub mod room;
