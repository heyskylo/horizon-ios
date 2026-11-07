//! The controller's menu cursor in inventories: a crosshair whose centre is
//! the point it clicks. Horizon's own art, packed with the button glyphs (`pad_glyph`).

/// The atlas sprite.
pub const SPRITE: &str = "horizon:gui/controller/crosshair_inv";
/// The size it's drawn at in GUI pixels (an item's); its texture is twice that.
pub const DRAWN: f32 = 16.0;

pub(crate) const PNG: &[u8] =
    include_bytes!("../../horizon-client/assets/horizon/textures/gui/controller/crosshair_inv.png");

/// Where to draw the cursor (top-left, GUI pixels) so its centre lands on
/// the GUI point `at`.
pub fn origin(at: (f32, f32)) -> (f32, f32) {
    (at.0 - DRAWN / 2.0, at.1 - DRAWN / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_is_a_crosshair_centred_on_its_point() {
        let (_, w, h, px) = crate::pad_glyph::images()
            .find(|i| i.0 == SPRITE)
            .expect("the cursor is packed");
        assert_eq!((w as f32, h as f32), (DRAWN * 2.0, DRAWN * 2.0));
        let alpha = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
        let middle = (w / 2 - 1)..=(w / 2);
        assert!(
            middle
                .clone()
                .any(|x| middle.clone().any(|y| alpha(x, y) > 0)),
            "the clicked point sits on the crosshair"
        );
        assert_eq!(alpha(0, 0), 0, "corners are clear: a plus, not a square");
        assert_eq!(origin((100.0, 50.0)), (92.0, 42.0));
    }
}
