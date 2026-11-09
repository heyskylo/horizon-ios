//! HUD boxes in the corners: the minimap's place and mods' panels, small
//! boxes in GUI pixels stacked per corner (`docs/extras.md`), and the layers crates above the
//! engine register (`horizon_hooks::hud`).
use crate::{
    extras::Corner,
    hud::{HudAtlas, HudBatch, Rect, rgba},
};
use horizon_hooks::hud;

/// The gap between an overlay and the screen edge or the next overlay.
const MARGIN: f32 = 2.0;
const BACKGROUND: u32 = 0x7800_0000;

/// Where the next overlay goes in each corner.
pub struct Corners {
    width: f32,
    height: f32,
    /// Distance from the corner's top or bottom edge, per `Corner::ALL`.
    used: [f32; 4],
}
impl Corners {
    /// `top_right` reserves the status effect icons' rows.
    pub fn new(width: f32, height: f32, top_right: f32) -> Self {
        Self {
            width,
            height,
            used: [0.0, top_right, 0.0, 0.0],
        }
    }
    /// The top-left position of a `w` x `h` box in `corner`, below (or
    /// above) what the corner already holds.
    pub fn place(&mut self, corner: Corner, w: f32, h: f32) -> (f32, f32) {
        let i = Corner::ALL.iter().position(|c| *c == corner).unwrap_or(0);
        let edge = self.used[i] + MARGIN;
        self.used[i] = edge + h;
        let x = if corner.left() {
            MARGIN
        } else {
            self.width - MARGIN - w
        };
        let y = if corner.top() {
            edge
        } else {
            self.height - edge - h
        };
        (x, y)
    }
}

/// How the minimap shows: its corner and side in GUI pixels, its image (an
/// atlas rect), the player arrow (the map item's decoration sprite) turned
/// `arrow` degrees clockwise from up, and the rim's N toward `north` when the
/// map turns.
pub struct MinimapLook {
    pub corner: Corner,
    pub size: f32,
    pub image: Rect,
    pub arrow: f32,
    pub north: Option<(f32, f32)>,
}
pub fn draw_minimap(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    corners: &mut Corners,
    look: &MinimapLook,
) {
    let MinimapLook {
        corner,
        size,
        image,
        arrow,
        north,
    } = *look;
    let (x, y) = corners.place(corner, size, size);
    batch.rect_uv(x, y, size, size, image, u32::MAX);
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    if let Some(r) = atlas.sprites.get("map/decorations/player").copied() {
        // MapRenderer's decorations draw the sprite flipped: its arrow
        // points down the texture, so up on screen is half a turn.
        let (s, c) = (arrow + 180.0).to_radians().sin_cos();
        let p = |dx: f32, dy: f32| [cx + dx * c - dy * s, cy + dx * s + dy * c];
        let h = 4.0;
        batch.quad(
            [p(-h, h), p(h, h), p(h, -h), p(-h, -h)],
            [[r.u0, r.v0], [r.u1, r.v0], [r.u1, r.v1], [r.u0, r.v1]],
            u32::MAX,
        );
    }
    if let Some((nx, ny)) = north {
        let reach = size / 2.0 - 6.0;
        let tw = HudBatch::text_width(atlas, "N");
        let (tx, ty) = (cx + nx * reach - tw / 2.0, cy + ny * reach - 4.0);
        batch.text(
            atlas,
            "N",
            tx.round(),
            ty.round(),
            rgba(255, 255, 255, 255),
            true,
        );
    }
}

/// A box of text lines (a mod's HUD panel): `(text, 0xRRGGBB)` each.
pub fn draw_lines(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    corners: &mut Corners,
    corner: Corner,
    lines: &[(&str, u32)],
) {
    if lines.is_empty() {
        return;
    }
    let w = lines
        .iter()
        .map(|(text, _)| HudBatch::text_width(atlas, text))
        .fold(0.0, f32::max)
        + 4.0;
    let h = 10.0 * lines.len() as f32 + 2.0;
    let (x, y) = corners.place(corner, w, h);
    batch.fill(atlas, x, y, w, h, BACKGROUND);
    for (i, (text, color)) in lines.iter().enumerate() {
        let [_, r, g, b] = color.to_be_bytes();
        let y = y + 2.0 + 10.0 * i as f32;
        batch.text(atlas, text, x + 2.0, y, rgba(r, g, b, 255), true);
    }
}

/// Draws the HUD layers crates registered, after the extras' and mods' boxes, which they stack
/// with. `shown`: whether each corner of `Corner::ALL` shows (the debug screen covers some).
pub fn draw_layers(
    layers: &[&dyn hud::Layer],
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    corners: &mut Corners,
    shown: [bool; 4],
) {
    let mut canvas = Canvas {
        batch,
        atlas,
        corners,
        shown,
    };
    for layer in layers {
        layer.draw(&mut canvas);
    }
}

struct Canvas<'a> {
    batch: &'a mut HudBatch,
    atlas: &'a HudAtlas,
    corners: &'a mut Corners,
    shown: [bool; 4],
}

/// `0xAARRGGBB` as the batch's colour.
fn argb_color(argb: u32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    rgba(r, g, b, a)
}

impl hud::Canvas for Canvas<'_> {
    fn size(&self) -> (f32, f32) {
        (self.batch.width, self.batch.height)
    }
    fn tr(&self, key: &str) -> String {
        crate::lang::get(key)
    }
    fn text_width(&self, text: &str) -> f32 {
        HudBatch::text_width(self.atlas, text)
    }
    fn text(&mut self, x: f32, y: f32, text: &str, argb: u32, shadow: bool) {
        self.batch
            .text(self.atlas, text, x, y, argb_color(argb), shadow);
    }
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        self.batch.fill(self.atlas, x, y, w, h, argb_color(argb));
    }
    fn place(&mut self, corner: hud::Corner, w: f32, h: f32) -> Option<(f32, f32)> {
        let corner = match corner {
            hud::Corner::TopLeft => Corner::TopLeft,
            hud::Corner::TopRight => Corner::TopRight,
            hud::Corner::BottomLeft => Corner::BottomLeft,
            hud::Corner::BottomRight => Corner::BottomRight,
        };
        let i = Corner::ALL.iter().position(|c| *c == corner)?;
        self.shown[i].then(|| self.corners.place(corner, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_stack_away_from_their_edge() {
        let mut c = Corners::new(400.0, 300.0, 26.0);
        assert_eq!(c.place(Corner::TopLeft, 50.0, 12.0), (2.0, 2.0));
        assert_eq!(c.place(Corner::TopLeft, 70.0, 20.0), (2.0, 16.0));
        // Top right starts under the effect icons.
        assert_eq!(c.place(Corner::TopRight, 50.0, 12.0), (348.0, 28.0));
        assert_eq!(c.place(Corner::BottomRight, 50.0, 12.0), (348.0, 286.0));
        assert_eq!(c.place(Corner::BottomRight, 50.0, 10.0), (348.0, 274.0));
        assert_eq!(c.place(Corner::BottomLeft, 10.0, 10.0), (2.0, 288.0));
    }

    /// Draws a box of one line in a corner and keeps where it went.
    struct Panel {
        corner: hud::Corner,
        at: std::sync::Mutex<Vec<Option<(f32, f32)>>>,
    }

    impl hud::Layer for Panel {
        fn draw(&self, canvas: &mut dyn hud::Canvas) {
            let w = canvas.text_width("12 CPS") + 4.0;
            let at = canvas.place(self.corner, w, 12.0);
            if let Some((x, y)) = at {
                canvas.fill(x, y, w, 12.0, 0x7800_0000);
                canvas.text(x + 2.0, y + 2.0, "12 CPS", 0xFFFF_FFFF, true);
            }
            self.at.lock().unwrap().push(at);
        }
    }

    #[test]
    fn layers_stack_with_the_extras_and_skip_hidden_corners() {
        let atlas = HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap();
        let mut batch = HudBatch::new([800, 600]);
        let (width, height) = (batch.width, batch.height);
        let mut corners = Corners::new(width, height, 0.0);
        // The minimap already took the top right.
        corners.place(Corner::TopRight, 64.0, 64.0);
        let right = Panel {
            corner: hud::Corner::TopRight,
            at: Default::default(),
        };
        let left = Panel {
            corner: hud::Corner::TopLeft,
            at: Default::default(),
        };
        let layers: [&dyn hud::Layer; 2] = [&right, &left];
        let empty = batch.vertices.len();
        draw_layers(&layers, &mut batch, &atlas, &mut corners, [false, true, true, true]);
        let (_, y) = right.at.lock().unwrap()[0].expect("the top right shows");
        assert_eq!(y, 2.0 + 64.0 + 2.0, "under the minimap");
        assert_eq!(*left.at.lock().unwrap(), [None], "the debug screen covers the top left");
        assert!(batch.vertices.len() > empty, "the shown box is drawn");
    }
}
