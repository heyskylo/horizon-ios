//! Icons: a small picture for an action or an entry, from the player's own resources. An icon is
//! a GUI sprite, an item, a controller glyph or the player's face. Sprites and glyphs draw here;
//! items and the face need the game (its item models, the player's skin), so the game paints
//! those through an [`IconPainter`] it implements (`game/item_draw.rs`). The render crate never
//! reaches gameplay, and tests use [`NoItems`].
//!
//! Every icon fills a square of `size` GUI px: 16 for a slot, 32 for a large one. A sprite of
//! another shape keeps its proportions inside the square, centred; an item model is drawn at 16
//! and scaled.
//!
//! Nothing here is bundled: sprites and items come from the vanilla resources the player
//! prepared, so a resource pack's art shows. A sprite the atlas lacks draws nothing (`draw`
//! returns `false`) and the overlay's text still says what the icon stood for.
//!
//! ```
//! use horizon_client_render::hud::{HudAtlas, HudBatch};
//! use horizon_client_render::hud_kit::icon::{self, Icon, NoItems};
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! let mut batch = HudBatch::new([854, 480]);
//! // Without the game, items have no painter: nothing is drawn.
//! let drawn = icon::draw(&mut batch, &atlas, &NoItems, Icon::Item("minecraft:chest"), [10.0, 10.0, 16.0], 1.0);
//! assert!(!drawn && batch.vertices.is_empty());
//! ```
use crate::hud::{HudAtlas, HudBatch};
use crate::pad_glyph::{self, Glyph};

/// The size, in GUI px, item models draw at before any scaling.
pub const ITEM_SIZE: f32 = 16.0;
/// Where GUI sprites sit in the atlas: `Icon::Sprite` names drop this prefix.
pub const SPRITES: &str = "gui/sprites/";

/// A small picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// A GUI sprite, by its name under `gui/sprites/` (`statistics/item_dropped`).
    Sprite(&'static str),
    /// An item's model, by its id (`minecraft:chest`).
    Item(&'static str),
    /// A controller button's glyph, in the player's glyphs.
    Glyph(Glyph),
    /// The player's own skin's face, with its hat layer.
    PlayerFace,
}

/// What draws the icons only the game can: items and the player's face. `area` is
/// `[x, y, size]` in GUI px, the square's top left and side.
pub trait IconPainter {
    /// Draws the item `id` filling `area`; `false` if it can't (an unknown id).
    fn item(&self, batch: &mut HudBatch, id: &str, area: [f32; 3]) -> bool;
    /// Draws the player's face filling `area`; `false` with no player or skin.
    fn face(&self, batch: &mut HudBatch, area: [f32; 3]) -> bool;
}

/// A painter with no game behind it: no items and no face. For tests and screens that show
/// sprites only.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoItems;

impl IconPainter for NoItems {
    fn item(&self, _: &mut HudBatch, _: &str, _: [f32; 3]) -> bool {
        false
    }
    fn face(&self, _: &mut HudBatch, _: [f32; 3]) -> bool {
        false
    }
}

/// The square, in GUI px, a `w` x `h` picture takes inside `[x, y, size]`: as large as fits
/// with its proportions kept, centred.
pub fn fit([x, y, size]: [f32; 3], w: f32, h: f32) -> [f32; 4] {
    if w <= 0.0 || h <= 0.0 {
        return [x, y, 0.0, 0.0];
    }
    let k = size / w.max(h);
    let (dw, dh) = (w * k, h * k);
    [x + (size - dw) / 2.0, y + (size - dh) / 2.0, dw, dh]
}

/// Draws `icon` filling `area` (`[x, y, size]`, GUI px), its alpha times `alpha` (0 to 1).
/// Whether anything was drawn.
pub fn draw(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    painter: &dyn IconPainter,
    icon: Icon,
    area: [f32; 3],
    alpha: f32,
) -> bool {
    let start = batch.vertices.len();
    let drawn = match icon {
        Icon::Sprite(name) => {
            let key = format!("{SPRITES}{name}");
            match atlas.sprites.get(&key) {
                Some(rect) => {
                    let [x, y, w, h] = fit(area, rect.w as f32, rect.h as f32);
                    batch.sprite_scaled(atlas, &key, x, y, w, h, u32::MAX);
                    true
                }
                None => false,
            }
        }
        Icon::Item(id) => painter.item(batch, id, area),
        Icon::Glyph(glyph) => {
            let [x, y, size] = area;
            let w = pad_glyph::width(atlas, glyph);
            // A glyph's rect starts a pixel left of and two above its text line.
            let gx = (x + (size - w) / 2.0).floor() + 1.0;
            let gy = (y + (size - pad_glyph::HEIGHT) / 2.0).floor() + 2.0;
            pad_glyph::draw(batch, atlas, glyph, gx, gy);
            true
        }
        Icon::PlayerFace => painter.face(batch, area),
    };
    batch.alpha_since(start, alpha);
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::MenuJob;
    use crate::pad_glyph::Glyphs;
    use std::cell::RefCell;

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    /// A painter that fills the area it is given and records what it was asked for.
    #[derive(Default)]
    struct Recorder(RefCell<Vec<(String, [f32; 3])>>);

    impl IconPainter for Recorder {
        fn item(&self, batch: &mut HudBatch, id: &str, area: [f32; 3]) -> bool {
            self.0.borrow_mut().push((id.to_owned(), area));
            batch.fill(&atlas(), area[0], area[1], area[2], area[2], u32::MAX);
            true
        }
        fn face(&self, _: &mut HudBatch, area: [f32; 3]) -> bool {
            self.0.borrow_mut().push(("face".to_owned(), area));
            true
        }
    }

    #[test]
    fn items_and_the_face_go_to_the_painter_and_are_faded() {
        let atlas = atlas();
        let painter = Recorder::default();
        let mut batch = HudBatch::new([854, 480]);
        assert!(draw(
            &mut batch,
            &atlas,
            &painter,
            Icon::Item("minecraft:feather"),
            [4.0, 6.0, 32.0],
            0.5
        ));
        assert!(draw(
            &mut batch,
            &atlas,
            &painter,
            Icon::PlayerFace,
            [0.0, 0.0, 16.0],
            1.0
        ));
        assert_eq!(
            *painter.0.borrow(),
            [
                ("minecraft:feather".to_owned(), [4.0, 6.0, 32.0]),
                ("face".to_owned(), [0.0, 0.0, 16.0])
            ]
        );
        assert!(batch.vertices.iter().all(|v| v.color >> 24 == 128));
    }

    #[test]
    fn without_a_painter_items_and_faces_draw_nothing() {
        let atlas = atlas();
        let mut batch = HudBatch::new([854, 480]);
        for icon in [Icon::Item("minecraft:chest"), Icon::PlayerFace] {
            assert!(!draw(
                &mut batch,
                &atlas,
                &NoItems,
                icon,
                [0.0, 0.0, 16.0],
                1.0
            ));
        }
        assert!(batch.vertices.is_empty());
    }

    #[test]
    fn a_sprite_the_atlas_lacks_draws_nothing() {
        let atlas = atlas();
        let mut batch = HudBatch::new([854, 480]);
        let icon = Icon::Sprite("statistics/no_such_sprite");
        assert!(!draw(
            &mut batch,
            &atlas,
            &NoItems,
            icon,
            [0.0, 0.0, 16.0],
            1.0
        ));
        assert!(batch.vertices.is_empty());
    }

    #[test]
    fn a_picture_keeps_its_proportions_centred_in_its_square() {
        assert_eq!(
            fit([10.0, 10.0, 16.0], 18.0, 18.0),
            [10.0, 10.0, 16.0, 16.0]
        );
        // hud/hotbar_offhand_left is 29 x 24.
        let [x, y, w, h] = fit([0.0, 0.0, 29.0], 29.0, 24.0);
        assert_eq!([x, w], [0.0, 29.0]);
        assert_eq!([y, h], [2.5, 24.0]);
        assert_eq!(fit([0.0, 0.0, 16.0], 0.0, 4.0)[2], 0.0);
    }

    #[test]
    fn a_glyph_is_centred_in_its_square() {
        let atlas = atlas();
        let mut batch = HudBatch::new([854, 480]);
        let glyph = Glyph::menu(MenuJob::Back, Glyphs::default());
        assert!(draw(
            &mut batch,
            &atlas,
            &NoItems,
            Icon::Glyph(glyph),
            [100.0, 100.0, 32.0],
            1.0
        ));
        let (lo, hi) = batch
            .vertices
            .iter()
            .fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), v| {
                (
                    [lo[0].min(v.pos[0]), lo[1].min(v.pos[1])],
                    [hi[0].max(v.pos[0]), hi[1].max(v.pos[1])],
                )
            });
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        assert!(
            (centre[0] - 116.0).abs() <= 1.5 && (centre[1] - 116.0).abs() <= 1.5,
            "{centre:?}"
        );
    }
}
