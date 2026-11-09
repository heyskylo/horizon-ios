//! Plates: the boxes Horizon's HUD overlays draw behind their text, one place for each look so
//! overlays that mean the same thing look the same.
//!
//! A plate is drawn first, then the overlay's text and glyphs over it. Only the kinds the
//! overlays use today are here; each keeps the colours its overlay always had. Today that is
//! one, shared by the controller notice and splitscreen's prompts.
//!
//! ```
//! use horizon_client_render::hud_kit::plate::Plate;
//!
//! // A notice's box: a dark fill and a faint 1 px edge inside its area.
//! assert_eq!(Plate::Notice.fill(), 0xD200_0000);
//! assert_eq!(Plate::Notice.edge(), Some(0x5AFF_FFFF));
//! ```
//!
//! In a draw: `plate::draw(batch, atlas, Plate::Notice, [x, y, w, h])`, the area in GUI px.

use crate::hud::{HudAtlas, HudBatch, rgba};

/// A plate's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plate {
    /// Something to read now: controller notices and splitscreen's prompts. A dark fill with a
    /// faint edge.
    Notice,
}

impl Plate {
    /// The fill colour as `0xAARRGGBB`.
    pub fn fill(self) -> u32 {
        match self {
            Self::Notice => 0xD200_0000,
        }
    }

    /// The 1 GUI px edge's colour as `0xAARRGGBB`, drawn inside the area; `None` for none.
    pub fn edge(self) -> Option<u32> {
        match self {
            Self::Notice => Some(0x5AFF_FFFF),
        }
    }
}

/// `[a, r, g, b]` of `0xAARRGGBB` as the batch's colour.
fn colour(argb: u32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    rgba(r, g, b, a)
}

/// Draws `plate` over `[x, y, w, h]` (GUI px): its fill, then its edge inside the area.
pub fn draw(batch: &mut HudBatch, atlas: &HudAtlas, plate: Plate, [x, y, w, h]: [f32; 4]) {
    batch.fill(atlas, x, y, w, h, colour(plate.fill()));
    if let Some(edge) = plate.edge() {
        let edge = colour(edge);
        for [ex, ey, ew, eh] in [
            [x, y, w, 1.0],
            [x, y + h - 1.0, w, 1.0],
            [x, y, 1.0, h],
            [x + w - 1.0, y, 1.0, h],
        ] {
            batch.fill(atlas, ex, ey, ew, eh, edge);
        }
    }
}
