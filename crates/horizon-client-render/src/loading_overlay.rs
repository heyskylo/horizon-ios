//! Vanilla's `LoadingOverlay`: the Mojang Studios screen over everything while the game loads, red (or black
//! with the Monochrome Logo option), the logo in the middle and a white progress bar below it. Once loading is
//! done it stays a second while the bar fades, then fades out itself over the next second.
use crate::hud::{HudAtlas, HudBatch, rgba};

/// `textures/gui/title/mojangstudios.png`: "MOJANG" in its top half, "STUDIOS" in its bottom half.
pub const LOGO: &str = "gui/title/mojangstudios";

/// `LoadingOverlay.BRAND_BACKGROUND` and the monochrome one.
const BRAND: (u8, u8, u8) = (239, 50, 61);
const MONOCHROME: (u8, u8, u8) = (0, 0, 0);

/// The overlay's state: the bar's eased progress and when loading finished.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    progress: f32,
    /// Seconds since loading finished, once it has.
    done_for: Option<f32>,
}

impl Overlay {
    /// One frame of `LoadingOverlay.render`'s easing: the bar moves a twentieth of the way to `actual`.
    pub fn ease(&mut self, actual: f32) {
        self.progress = (self.progress * 0.95 + actual * 0.05).clamp(0.0, 1.0);
    }

    /// Loading finished `seconds` ago.
    pub fn done(&mut self, seconds: f32) {
        self.done_for = Some(seconds);
    }

    /// Both seconds of the fade are over: nothing to draw again.
    pub fn finished(&self) -> bool {
        self.done_for.is_some_and(|t| t >= 2.0)
    }

    /// Draw it over whatever the batch holds.
    pub fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas, monochrome: bool) {
        // fadeOutAnim: the overlay's own alpha drops over the second second, the bar's over the first.
        let fade = self.done_for.unwrap_or(-1.0);
        let alpha = if fade >= 1.0 {
            1.0 - (fade - 1.0).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if alpha <= 0.0 {
            return;
        }
        let (w, h) = (batch.width, batch.height);
        let (r, g, b) = if monochrome { MONOCHROME } else { BRAND };
        let a = (alpha * 255.0).round() as u8;
        batch.fill(atlas, 0.0, 0.0, w, h, rgba(r, g, b, a));
        // The logo: two halves side by side, each half the logo's width.
        let logo_height = (f64::from(w) * 0.75).min(f64::from(h)) * 0.25;
        let half_height = (logo_height * 0.5) as i32 as f32;
        let half_width = (logo_height * 4.0 * 0.5) as i32 as f32;
        let (cx, cy) = ((w / 2.0).floor(), (h / 2.0).floor());
        if let Some(sprite) = atlas.sprites.get(LOGO).copied() {
            let mid = (sprite.v0 + sprite.v1) / 2.0;
            let (mut top, mut bottom) = (sprite, sprite);
            top.v1 = mid;
            bottom.v0 = mid;
            let white = rgba(255, 255, 255, a);
            let y = cy - half_height;
            let height = logo_height as i32 as f32;
            batch.rect_uv(cx - half_width, y, half_width, height, top, white);
            batch.rect_uv(cx, y, half_width, height, bottom, white);
        }
        // drawProgressBar, fading over the first second after loading.
        if fade < 1.0 {
            let bar_alpha = 1.0 - fade.clamp(0.0, 1.0);
            let bar_y = (f64::from(h) * 0.8325) as i32 as f32;
            let (x0, y0, x1, y1) = (cx - half_width, bar_y - 5.0, cx + half_width, bar_y + 5.0);
            let white = rgba(255, 255, 255, (bar_alpha * alpha * 255.0).round() as u8);
            let fill = ((x1 - x0 - 2.0) * self.progress).ceil();
            batch.fill(
                atlas,
                x0 + 2.0,
                y0 + 2.0,
                (fill - 2.0).max(0.0),
                y1 - y0 - 4.0,
                white,
            );
            batch.fill(atlas, x0 + 1.0, y0, x1 - x0 - 2.0, 1.0, white);
            batch.fill(atlas, x0 + 1.0, y1 - 1.0, x1 - x0 - 2.0, 1.0, white);
            batch.fill(atlas, x0, y0, 1.0, y1 - y0, white);
            batch.fill(atlas, x1 - 1.0, y0, 1.0, y1 - y0, white);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_eases_toward_the_real_progress() {
        let mut o = Overlay::default();
        o.ease(1.0);
        assert!((o.progress - 0.05).abs() < 1e-6);
        for _ in 0..200 {
            o.ease(1.0);
        }
        assert!(o.progress > 0.99 && o.progress <= 1.0);
    }

    #[test]
    fn it_stays_a_second_then_fades_for_one() {
        let mut o = Overlay::default();
        assert!(!o.finished());
        o.done(1.5);
        assert!(!o.finished());
        o.done(2.0);
        assert!(o.finished());
    }
}
