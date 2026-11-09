//! Hint rows: a controller's glyphs and what pressing them does, "(A) Jump". The gameplay guide,
//! the controller notice, splitscreen's prompts and the on-screen keyboard all show them; they
//! measure and draw them here, so their spacing and their fitting are the same everywhere.
//!
//! A row is its glyphs (`pad_glyph::Glyph`, already in the player's glyphs and letters) and its
//! caption. A [`Look`] says how it reads: the caption's colour and shadow, the gaps, and which
//! side the glyphs go. All sizes are GUI px.
//!
//! ```
//! use horizon_client_render::gamepad::MenuJob;
//! use horizon_client_render::hud::HudAtlas;
//! use horizon_client_render::hud_kit::hint::{self, HintRow};
//! use horizon_client_render::pad_glyph::{Glyph, Glyphs};
//!
//! let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
//! let back = Glyph::menu(MenuJob::Back, Glyphs::default());
//! let row = HintRow::new([back], "Leave");
//! // Its glyph, the gap, then the caption.
//! let wide = row.measure(&atlas, &hint::NOTICE);
//! assert_eq!(wide, row.glyphs_width(&atlas, &hint::NOTICE) + 5.0 + atlas.font.width("Leave"));
//! // Cut to a narrower room, its caption ellipsizes and its glyph stays.
//! let fitted = row.fitted(&atlas, &hint::NOTICE, wide - 8.0);
//! assert!(fitted.measure(&atlas, &hint::NOTICE) <= wide - 8.0);
//! assert_eq!(fitted.glyphs, row.glyphs);
//! ```
//!
//! In a draw: `hint::draw_hint(batch, atlas, &row, &look, x, y)` at the row's left edge and its
//! text line's top, or `hint::draw_plated` for a row on its own plate, as the guide's are.

use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::hud_kit::fit;
use crate::hud_kit::plate::{self, Plate};
use crate::pad_glyph::{self, Glyph};

/// Which side of the caption the glyphs go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Glyphs, then the caption: "(A) Jump".
    Leading,
    /// The caption, then the glyphs, for a row against the view's right edge: "Break (RT)".
    Trailing,
}

/// How a hint row reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The caption's colour, as the batch takes it (`hud::rgba`).
    pub color: u32,
    /// The caption has a text shadow.
    pub shadow: bool,
    /// GUI px between the glyphs' art and the caption.
    pub gap: f32,
    /// GUI px between two glyphs held together ("(LT) + (A)").
    pub between: f32,
    /// Where the glyphs go.
    pub side: Side,
}

/// The gameplay guide's rows: white with a shadow, glyphs close together. A row in the right
/// column takes `Side::Trailing`.
pub const GUIDE: Look = Look {
    color: u32::MAX,
    shadow: true,
    gap: 3.0,
    between: 1.0,
    side: Side::Leading,
};

/// A notice's line under its title: light grey, no shadow.
pub const NOTICE: Look = Look {
    color: rgba(200, 200, 200, 255),
    shadow: false,
    gap: 5.0,
    between: 1.0,
    side: Side::Leading,
};

/// The answers of a splitscreen prompt: a little lighter than a notice's line.
pub const PROMPT: Look = Look {
    color: rgba(220, 220, 220, 255),
    ..NOTICE
};

/// GUI px of plate a plated row keeps on its caption's side (`draw_plated`); the glyphs' art
/// reaches the plate's other edge.
pub const PAD: f32 = 3.0;

/// GUI px from a plated row's top to its text line: a glyph's art reaches 2 px above the line
/// (`pad_glyph::rect`), one past the plate, into the gap above it.
pub const LINE_DROP: f32 = 1.0;

/// Glyphs and what pressing them does.
#[derive(Clone, Debug, PartialEq)]
pub struct HintRow {
    /// The buttons, held together in this order; none shows the caption alone.
    pub glyphs: Vec<Glyph>,
    /// What they do, translated.
    pub caption: String,
}

impl HintRow {
    pub fn new(glyphs: impl IntoIterator<Item = Glyph>, caption: impl Into<String>) -> Self {
        Self {
            glyphs: glyphs.into_iter().collect(),
            caption: caption.into(),
        }
    }

    /// How wide the glyphs show side by side, `look.between` apart.
    pub fn glyphs_width(&self, atlas: &HudAtlas, look: &Look) -> f32 {
        let art: f32 = self
            .glyphs
            .iter()
            .map(|&g| pad_glyph::width(atlas, g))
            .sum();
        art + look.between * self.glyphs.len().saturating_sub(1) as f32
    }

    /// The gap between the glyphs and the caption: none unless there are both.
    fn gap(&self, look: &Look) -> f32 {
        if self.glyphs.is_empty() || self.caption.is_empty() {
            0.0
        } else {
            look.gap
        }
    }

    /// How wide the row draws: glyphs, gap and caption.
    pub fn measure(&self, atlas: &HudAtlas, look: &Look) -> f32 {
        self.glyphs_width(atlas, look) + self.gap(look) + atlas.font.width(&self.caption)
    }

    /// The row in at most `width`: its caption [ellipsized](fit::ellipsize) to the room its
    /// glyphs leave. The glyphs never go; with no room for them the row is still wider.
    pub fn fitted(&self, atlas: &HudAtlas, look: &Look, width: f32) -> HintRow {
        let room = width - self.glyphs_width(atlas, look) - self.gap(look);
        HintRow {
            glyphs: self.glyphs.clone(),
            caption: fit::ellipsize(atlas, &self.caption, room).into_owned(),
        }
    }

    /// The row as plain text, each glyph by its printed label ("A Type"), for a surface that
    /// draws no glyph art (the on-screen keyboard's own font).
    pub fn text(&self) -> String {
        let labels = self.glyphs.iter().map(|g| g.label());
        labels
            .chain((!self.caption.is_empty()).then_some(self.caption.as_str()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Draws `row` from `x`, its left edge (where the first glyph's art starts with `Side::Leading`,
/// `pad_glyph::rect`), on the text line whose top is `y`. Returns how wide it drew (`measure`).
pub fn draw_hint(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    row: &HintRow,
    look: &Look,
    x: f32,
    y: f32,
) -> f32 {
    let caption = atlas.font.width(&row.caption);
    let (glyphs_x, caption_x) = match look.side {
        Side::Leading => (x, x + row.glyphs_width(atlas, look) + row.gap(look)),
        Side::Trailing => (x + caption + row.gap(look), x),
    };
    // `pad_glyph::draw` takes the text line's origin; its art starts 1 px left of it.
    let mut gx = glyphs_x + 1.0;
    for &glyph in &row.glyphs {
        pad_glyph::draw(batch, atlas, glyph, gx, y);
        gx += pad_glyph::width(atlas, glyph) + look.between;
    }
    if !row.caption.is_empty() {
        batch.text(atlas, &row.caption, caption_x, y, look.color, look.shadow);
    }
    row.measure(atlas, look)
}

/// How wide `row` is on its own plate: its width and [`PAD`].
pub fn plated_width(atlas: &HudAtlas, row: &HintRow, look: &Look) -> f32 {
    row.measure(atlas, look) + PAD
}

/// Draws `plate` over `area` (`[x, y, w, h]`) and `row` on it: against the area's left edge with
/// `Side::Leading`, its right with `Side::Trailing`, its text line [`LINE_DROP`] below the top.
pub fn draw_plated(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    row: &HintRow,
    look: &Look,
    plate: Plate,
    area @ [x, y, w, _]: [f32; 4],
) {
    plate::draw(batch, atlas, plate, area);
    let left = match look.side {
        Side::Leading => x,
        Side::Trailing => x + w - row.measure(atlas, look),
    };
    draw_hint(batch, atlas, row, look, left, y + LINE_DROP);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::{GlyphStyle, MenuJob, PadSettings};
    use crate::pad_glyph::Glyphs;

    fn atlas() -> HudAtlas {
        HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap()
    }

    fn glyph(job: MenuJob) -> Glyph {
        Glyph::menu(job, Glyphs::of(&PadSettings::default(), GlyphStyle::Xbox))
    }

    fn batch() -> HudBatch {
        HudBatch::new([400, 300])
    }

    /// The vertices' positions and colours: what the frame shows.
    fn drawn(batch: &HudBatch) -> Vec<([f32; 2], u32)> {
        batch.vertices.iter().map(|v| (v.pos, v.color)).collect()
    }

    #[test]
    fn a_leading_row_is_its_glyphs_then_its_caption() {
        let atlas = atlas();
        let (lt, a) = (glyph(MenuJob::PagePrev), glyph(MenuJob::Confirm));
        let row = HintRow::new([lt, a], "Craft");
        let mut got = batch();
        let drew = draw_hint(&mut got, &atlas, &row, &GUIDE, 50.0, 20.0);
        // By hand: each glyph's art from the edge (`pad_glyph::draw` 1 px right of it), 1 px
        // apart, then the caption 3 px past the art.
        let mut want = batch();
        let (lw, aw) = (pad_glyph::width(&atlas, lt), pad_glyph::width(&atlas, a));
        pad_glyph::draw(&mut want, &atlas, lt, 51.0, 20.0);
        pad_glyph::draw(&mut want, &atlas, a, 51.0 + lw + 1.0, 20.0);
        want.text(
            &atlas,
            "Craft",
            50.0 + lw + 1.0 + aw + 3.0,
            20.0,
            u32::MAX,
            true,
        );
        assert_eq!(drawn(&got), drawn(&want));
        assert_eq!(drew, lw + 1.0 + aw + 3.0 + atlas.font.width("Craft"));
    }

    #[test]
    fn a_trailing_row_is_its_caption_then_its_glyphs() {
        let atlas = atlas();
        let b = glyph(MenuJob::Back);
        let row = HintRow::new([b], "Leave");
        let look = Look {
            side: Side::Trailing,
            ..NOTICE
        };
        let mut got = batch();
        draw_hint(&mut got, &atlas, &row, &look, 10.0, 10.0);
        let mut want = batch();
        let caption = atlas.font.width("Leave");
        pad_glyph::draw(&mut want, &atlas, b, 10.0 + caption + 5.0 + 1.0, 10.0);
        want.text(&atlas, "Leave", 10.0, 10.0, NOTICE.color, false);
        assert_eq!(drawn(&got), drawn(&want));
    }

    #[test]
    fn a_fitted_row_keeps_its_glyphs_and_fits_its_room() {
        let atlas = atlas();
        let row = HintRow::new([glyph(MenuJob::Confirm)], "Release to Shoot");
        let whole = row.measure(&atlas, &GUIDE);
        assert_eq!(row.fitted(&atlas, &GUIDE, whole), row);
        // Down to the narrowest that still shows the dots.
        let least = row.glyphs_width(&atlas, &GUIDE) + GUIDE.gap + atlas.font.width(fit::ELLIPSIS);
        for room in [whole - 1.0, whole - 20.0, least] {
            let fitted = row.fitted(&atlas, &GUIDE, room);
            assert_eq!(fitted.glyphs, row.glyphs);
            assert!(
                fitted.caption.ends_with(fit::ELLIPSIS),
                "{}",
                fitted.caption
            );
            assert!(
                fitted.measure(&atlas, &GUIDE) <= room,
                "{} in {room}",
                fitted.caption
            );
        }
    }

    #[test]
    fn a_plated_row_keeps_its_pad_on_the_caption_side() {
        let atlas = atlas();
        let a = glyph(MenuJob::Confirm);
        let row = HintRow::new([a], "Jump");
        let w = plated_width(&atlas, &row, &GUIDE);
        let plate = Plate::Backing { alpha: 102 };
        let area = [100.0, 40.0, w, 11.0];
        // Leading: the glyph's art at the plate's left edge; trailing, the caption 3 px in.
        for (side, left) in [(Side::Leading, 100.0), (Side::Trailing, 103.0)] {
            let look = Look { side, ..GUIDE };
            let mut got = batch();
            draw_plated(&mut got, &atlas, &row, &look, plate, area);
            let mut want = batch();
            plate::draw(&mut want, &atlas, plate, area);
            draw_hint(&mut want, &atlas, &row, &look, left, 41.0);
            assert_eq!(drawn(&got), drawn(&want), "{side:?}");
        }
    }

    /// What each overlay drew before the kit, verbatim but for its inputs, against the kit's
    /// draw with the same inputs: rows that fit look exactly as they did.
    #[test]
    fn the_migrated_overlays_draw_what_they_drew_before() {
        let atlas = atlas();
        let (lt, a) = (glyph(MenuJob::PagePrev), glyph(MenuJob::Confirm));
        let backing = rgba(0, 0, 0, 102);
        let width = |text: &str| HudBatch::text_width(&atlas, text).ceil();
        for (with, caption) in [
            (None, "Jump"),
            (Some(lt), "Craft All"),
            (None, "Inventar öffnen"),
        ] {
            // The gameplay guide's row (game/pad/guide.rs), in either column.
            let keys = with.map_or(0.0, |w| pad_glyph::width(&atlas, w) + 1.0)
                + pad_glyph::width(&atlas, a);
            let w = keys + width(caption) + 6.0;
            for left in [true, false] {
                let [x, y, h] = [40.0, 30.0, 11.0];
                let mut old = batch();
                let (key_x, text_x) = if left {
                    (x, x + keys + 3.0)
                } else {
                    (x + w - keys, x + 3.0)
                };
                old.fill(&atlas, x, y, w, h, backing);
                let mut glyph_x = key_x + 1.0;
                if let Some(with) = with {
                    pad_glyph::draw(&mut old, &atlas, with, glyph_x, y + 1.0);
                    glyph_x += pad_glyph::width(&atlas, with) + 1.0;
                }
                pad_glyph::draw(&mut old, &atlas, a, glyph_x, y + 1.0);
                old.text(&atlas, caption, text_x, y + 1.0, u32::MAX, true);

                let row = HintRow::new(with.into_iter().chain([a]), caption);
                let side = if left { Side::Leading } else { Side::Trailing };
                let look = Look { side, ..GUIDE };
                assert_eq!(plated_width(&atlas, &row, &look), w);
                let mut new = batch();
                draw_plated(
                    &mut new,
                    &atlas,
                    &row,
                    &look,
                    Plate::Backing { alpha: 102 },
                    [x, y, w, h],
                );
                assert_eq!(
                    drawn(&new),
                    drawn(&old),
                    "guide row {caption:?}, left {left}"
                );
            }

            // The controller notice's first line (controller_notice.rs) and a splitscreen
            // prompt's answer (game/split/hud.rs): the glyph at `lx`, the words past it.
            for (look, grey) in [(NOTICE, 200), (PROMPT, 220)] {
                let lx = 77.0;
                let mut old = batch();
                pad_glyph::draw(&mut old, &atlas, a, lx, 50.0);
                let indent = pad_glyph::width(&atlas, a) + 4.0;
                old.text(
                    &atlas,
                    caption,
                    lx + indent,
                    50.0,
                    rgba(grey, grey, grey, 255),
                    false,
                );
                let mut new = batch();
                let row = HintRow::new([a], caption);
                draw_hint(&mut new, &atlas, &row, &look, lx - 1.0, 50.0);
                assert_eq!(drawn(&new), drawn(&old), "{caption:?} with {look:?}");
            }
        }

        // The on-screen keyboard's hint line (osk.rs): each button by its print, then its words.
        let old = format!("{} {}", a.label(), "Type");
        assert_eq!(HintRow::new([a], "Type").text(), old);
    }

    #[test]
    fn a_text_row_names_its_buttons_by_their_print() {
        let row = HintRow::new([glyph(MenuJob::PagePrev), glyph(MenuJob::Confirm)], "Craft");
        assert_eq!(row.text(), "LB A Craft");
        assert_eq!(HintRow::new([], "Type").text(), "Type");
    }
}
