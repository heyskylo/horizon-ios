//! The HUD kit: small immediate-mode pieces Horizon's own overlays share, so they fit around
//! vanilla's HUD and look alike (`docs/ui-kit.md`). Nothing here reaches gameplay; the game
//! builds the inputs (`Game::hud_room`, the frame's `motion::Clock`, an `icon::IconPainter`)
//! and the overlays draw with them.
//!
//! - `room`: what of a view an overlay may use, clear of the boss bars.
//! - `plate`: the box behind a notice's text.
//! - `fit`: text cut, wrapped or fitted in a circle to the room it has.
//! - `hint`: a controller's glyphs and what they do, "(A) Jump".
//! - `shapes`: triangles, fans, arcs and discs for `HudBatch`, and fades and moves of what was
//!   drawn since a point.
//! - `motion`: quiet tweens as pure functions of the frame's clock.
//! - `icon`: a sprite, an item, a glyph or the player's face as a small picture.
//! - `tabs`: a collapsed page strip, the name between arrows with a pip per page.
//!
//! Vanilla's own display lists (`hud_layout`, `screen_draw`) never use the kit: they stay
//! exactly vanilla's.
pub mod fit;
pub mod hint;
pub mod icon;
pub mod motion;
pub mod plate;
pub mod room;
pub mod shapes;
pub mod tabs;

/// `0xRRGGBB` and an alpha as the batch's colour (`hud::rgba`), for named colour constants.
pub const fn colour(rgb: u32, alpha: u8) -> u32 {
    (alpha as u32) << 24 | (rgb & 0xFF) << 16 | (rgb >> 8 & 0xFF) << 8 | (rgb >> 16 & 0xFF)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_named_colour_is_the_batch_s_rgba() {
        assert_eq!(
            super::colour(0x7656FA, 140),
            crate::hud::rgba(0x76, 0x56, 0xFA, 140)
        );
    }
}
