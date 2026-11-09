//! The emote wheel (`key.emote`, B by default; bindable on a controller): the player's emotes
//! around the middle of the screen. Hover one and release the key, click it, or press its
//! number; Escape or the key again closes it. The game doesn't pause and nothing is sent to the
//! game server.

use crate::game::{Game, Screen};
use horizon_client::{hud::HudBatch, text::Text};
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Emotes on the wheel.
pub(in crate::game) const SLOTS: usize = 8;
/// The ring, in GUI pixels from the middle; inside `INNER` nothing is hovered.
const INNER: f32 = 30.0;
const OUTER: f32 = 92.0;
/// Held at least this long, releasing the key plays what is hovered (or closes); a tap leaves the
/// wheel open.
const HOLD: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(in crate::game) struct Wheel {
    opened: Option<Instant>,
    hovered: Option<usize>,
}

/// The slice under a point `(dx, dy)` from the middle (GUI pixels, y down) of a wheel of `n`:
/// slice 0 is straight up, then clockwise. None in the dead middle or past the ring.
pub(in crate::game) fn slice_at(dx: f32, dy: f32, n: usize) -> Option<usize> {
    let r = dx.hypot(dy);
    if n == 0 || r < INNER || r > OUTER * 1.6 {
        return None;
    }
    let turn = std::f32::consts::TAU / n as f32;
    // Clockwise from up, centred on each slice.
    let angle = (dx.atan2(-dy) + turn / 2.0).rem_euclid(std::f32::consts::TAU);
    Some(((angle / turn) as usize).min(n - 1))
}

/// A point at `radius` and `angle` (radians clockwise from up) around `(cx, cy)`.
fn at(cx: f32, cy: f32, radius: f32, angle: f32) -> [f32; 2] {
    [cx + radius * angle.sin(), cy - radius * angle.cos()]
}

impl Game<'_> {
    /// Opens the wheel from play.
    pub(in crate::game) fn open_emote_wheel(&mut self) {
        self.refresh_owned_emotes();
        self.cosmetics.wheel = Wheel {
            opened: Some(Instant::now()),
            hovered: None,
        };
        self.screen = Screen::EmoteWheel;
        self.keys.clear();
        self.left = false;
        self.right = false;
        self.mining = None;
        self.capture(false);
    }

    /// Closes the wheel; `play`: the slice to play first.
    pub(in crate::game) fn close_emote_wheel(&mut self, play: Option<usize>) {
        self.cosmetics.wheel = Wheel::default();
        self.screen = Screen::Playing;
        self.capture(true);
        let id = play.and_then(|i| self.owned_emotes().get(i).map(|e| e.id.clone()));
        if let Some(id) = id {
            self.play_emote(&id);
        }
    }

    /// The emote key let go: a held press plays what is hovered, a tap keeps the wheel open.
    pub(in crate::game) fn emote_wheel_released(&mut self) {
        let held = self
            .cosmetics
            .wheel
            .opened
            .is_some_and(|at| at.elapsed() >= HOLD);
        if held || self.cosmetics.wheel.hovered.is_some() {
            self.close_emote_wheel(self.cosmetics.wheel.hovered);
        }
    }

    /// A window event while the wheel is open: the emote key let go, or a left click. True when
    /// the wheel took it.
    pub(in crate::game) fn emote_wheel_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Released
                    && matches!(event.physical_key, PhysicalKey::Code(key)
                        if self.settings.keys.translate(key) == Some(Ok(KeyCode::KeyB))) =>
            {
                self.emote_wheel_released();
                true
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.emote_wheel_click();
                true
            }
            _ => false,
        }
    }

    /// A click on the wheel: plays the hovered emote, or closes.
    pub(in crate::game) fn emote_wheel_click(&mut self) {
        self.close_emote_wheel(self.cosmetics.wheel.hovered);
    }

    pub(in crate::game) fn draw_emote_wheel(&mut self, batch: &mut HudBatch) {
        horizon_client::widgets::begin_screen(|| {
            horizon_client::widgets::ScreenInfo::new("emote_wheel", "horizon.emote_wheel.title")
        });
        let ids: Vec<String> = self
            .owned_emotes()
            .iter()
            .take(SLOTS)
            .map(|e| e.id.clone())
            .collect();
        let names: Vec<String> = ids.iter().map(|id| self.emote_name(id)).collect();
        let (w, h) = (batch.width, batch.height);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let n = names.len();
        // The cursor is in GUI pixels, as the batch.
        let (mx, my) = self.cursor;
        self.cosmetics.wheel.hovered = slice_at(mx - cx, my - cy, n);
        let hovered = self.cosmetics.wheel.hovered;
        let atlas = &self.gui.atlas;
        // The ring: each slice a fan of quads, the hovered one brighter.
        let slices = n.max(1);
        let turn = std::f32::consts::TAU / slices as f32;
        let gap = 0.03;
        for i in 0..slices {
            let colour = match (n, hovered == Some(i)) {
                (0, _) => 0x9010_1018,
                (_, true) => 0xD0A9_8FFF,
                _ => 0xA010_1018,
            };
            let from = i as f32 * turn - turn / 2.0 + gap;
            let span = turn - 2.0 * gap;
            for k in 0..6 {
                let (a0, a1) = (
                    from + span * k as f32 / 6.0,
                    from + span * (k + 1) as f32 / 6.0,
                );
                batch.fill_quad(
                    atlas,
                    [
                        at(cx, cy, INNER, a0),
                        at(cx, cy, OUTER, a0),
                        at(cx, cy, OUTER, a1),
                        at(cx, cy, INNER, a1),
                    ],
                    colour,
                );
            }
        }
        // Each slice's number and name, in the middle of its band.
        for (i, name) in names.iter().enumerate() {
            let [x, y] = at(cx, cy, (INNER + OUTER) / 2.0, i as f32 * turn);
            let label = format!("{} {name}", i + 1);
            let lw = HudBatch::text_width(atlas, &label);
            let colour = if hovered == Some(i) {
                u32::MAX
            } else {
                0xFFC8_C8D0
            };
            batch.text(atlas, &label, x - lw / 2.0, y - 4.0, colour, true);
        }
        let title = match (n, hovered) {
            (0, _) => Text::translate("horizon.emote_wheel.empty", &[]),
            (_, Some(i)) => Text::literal(&names[i]),
            _ => Text::translate("horizon.emote_wheel.title", &[]),
        };
        let plain = title.plain();
        let tw = HudBatch::text_width(atlas, &plain);
        batch.text_styled(atlas, &title, cx - tw / 2.0, cy - 4.0, u32::MAX, true);
        let help = Text::translate("horizon.emote_wheel.help", &[]);
        let hw = HudBatch::text_width(atlas, &help.plain());
        batch.text_styled(
            atlas,
            &help,
            cx - hw / 2.0,
            cy + OUTER + 10.0,
            0xFFA0_A0A8,
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_go_clockwise_from_the_top_with_a_dead_middle() {
        // Eight slices: up, up-right, right... each centred on its direction.
        assert_eq!(slice_at(0.0, -60.0, 8), Some(0));
        assert_eq!(slice_at(42.0, -42.0, 8), Some(1));
        assert_eq!(slice_at(60.0, 0.0, 8), Some(2));
        assert_eq!(slice_at(0.0, 60.0, 8), Some(4));
        assert_eq!(slice_at(-60.0, 0.0, 8), Some(6));
        // Just left of up is still the top slice; a bit more is the last.
        assert_eq!(slice_at(-10.0, -60.0, 8), Some(0));
        assert_eq!(slice_at(-35.0, -60.0, 8), Some(7));
        // The middle and far away: nothing. No emotes: nothing.
        assert_eq!(slice_at(5.0, 5.0, 8), None);
        assert_eq!(slice_at(0.0, -400.0, 8), None);
        assert_eq!(slice_at(0.0, -60.0, 0), None);
        // Two: top and bottom halves.
        assert_eq!(slice_at(50.0, -10.0, 2), Some(0));
        assert_eq!(slice_at(50.0, 10.0, 2), Some(1));
    }
}
