//! The emote wheel (`key.emote`, B by default; bindable on a controller): the player's emotes
//! around the middle of the screen. Hover one and release the key, click it, or press its
//! number; Escape or the key again closes it. The game doesn't pause and nothing is sent to the
//! game server.

use crate::game::{Game, Screen};
use horizon_client::gamepad::{Button, PadState};
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
    return_to: Option<Screen>,
    displayed: Vec<String>,
    controller: Option<(Button, u64)>,
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

fn wheel_scale(width: f32, height: f32) -> f32 {
    ((width - 32.0) / (OUTER * 2.0))
        .min((height - 64.0) / (OUTER * 2.0))
        .clamp(0.25, 1.0)
}

impl Game<'_> {
    /// A wheel reached from the pause menu keeps that pause until an emote
    /// is selected. The gameplay shortcut remains a non-pausing overlay.
    pub(in crate::game) fn emote_wheel_pauses(&self) -> bool {
        self.screen == Screen::EmoteWheel && self.cosmetics.wheel.return_to == Some(Screen::Paused)
    }

    /// Opens the wheel from play.
    pub(in crate::game) fn open_emote_wheel(&mut self) {
        self.refresh_owned_emotes();
        self.release_use_for_keybind_screen();
        let return_to = (self.screen == Screen::Paused).then_some(Screen::Paused);
        self.cosmetics.wheel = Wheel {
            opened: Some(Instant::now()),
            hovered: None,
            return_to,
            ..Wheel::default()
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
        let wheel = std::mem::take(&mut self.cosmetics.wheel);
        let id = play.and_then(|i| wheel.displayed.get(i).cloned());
        self.screen = if id.is_some() {
            Screen::Playing
        } else {
            wheel.return_to.unwrap_or(Screen::Playing)
        };
        self.capture(self.screen == Screen::Playing);
        if let Some(id) = id {
            self.play_emote(&id);
        }
    }

    /// Track the actual opening button outside cancelled gameplay mappings:
    /// opening a menu deliberately releases those mappings before the real pad lets go.
    pub(in crate::game) fn emote_controller_opened(&mut self, button: Button) {
        self.cosmetics.wheel.controller = Some((button, self.pad.owner_epoch));
    }

    pub(in crate::game) fn poll_emote_controller(&mut self, state: &PadState) {
        let Some((button, owner)) = self.cosmetics.wheel.controller else {
            return;
        };
        if self.screen != Screen::EmoteWheel || owner != self.pad.owner_epoch || !self.pad_focused()
        {
            self.cosmetics.wheel.controller = None;
            return;
        }
        let held = match button {
            Button::LeftTrigger => {
                state.triggers[0] >= (self.settings.controller.trigger_threshold - 0.1).max(0.05)
            }
            Button::RightTrigger => {
                state.triggers[1] >= (self.settings.controller.trigger_threshold - 0.1).max(0.05)
            }
            _ => state.held(button),
        };
        if !held {
            self.cosmetics.wheel.controller = None;
            self.emote_wheel_released();
        }
    }

    pub(in crate::game) fn cancel_emote_controller(&mut self) {
        self.cosmetics.wheel.controller = None;
    }

    fn emote_pointer_slot(&self) -> Option<usize> {
        let (w, h) = self.gui_size();
        let scale = wheel_scale(w, h);
        slice_at(
            (self.cursor.0 - w / 2.0) / scale,
            (self.cursor.1 - h / 2.0) / scale,
            self.cosmetics.wheel.displayed.len(),
        )
    }

    /// The emote key let go: a held press plays what is hovered, a tap keeps the wheel open.
    pub(in crate::game) fn emote_wheel_released(&mut self) {
        let held = self
            .cosmetics
            .wheel
            .opened
            .is_some_and(|at| at.elapsed() >= HOLD);
        let choice = self.emote_pointer_slot();
        if held || choice.is_some() {
            self.close_emote_wheel(choice);
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
                // Continue ordinary release bookkeeping (including rebound mouse actions).
                false
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
        self.close_emote_wheel(self.emote_pointer_slot());
    }

    pub(in crate::game) fn draw_emote_wheel(&mut self, batch: &mut HudBatch) {
        self.refresh_owned_emotes();
        let load_status = self.cosmetics.emotes.catalogue.status();
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
        self.cosmetics.wheel.displayed = ids.clone();
        let (w, h) = (batch.width, batch.height);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let scale = wheel_scale(w, h);
        let n = names.len();
        // The cursor is in GUI pixels, as the batch.
        let (mx, my) = self.cursor;
        self.cosmetics.wheel.hovered = slice_at((mx - cx) / scale, (my - cy) / scale, n);
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
                        at(cx, cy, INNER * scale, a0),
                        at(cx, cy, OUTER * scale, a0),
                        at(cx, cy, OUTER * scale, a1),
                        at(cx, cy, INNER * scale, a1),
                    ],
                    colour,
                );
            }
        }
        // Each slice's number and name, in the middle of its band.
        for (i, name) in names.iter().enumerate() {
            let [x, y] = at(cx, cy, (INNER + OUTER) / 2.0 * scale, i as f32 * turn);
            let rect = [
                x - 14.0 * scale,
                y - 10.0 * scale,
                28.0 * scale,
                20.0 * scale,
            ];
            horizon_client::widgets::record(name, rect, true);
            horizon_client::widgets::pointer_binding(rect, format!("emote:{}", ids[i]));
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
            (0, _) => Text::translate(load_status.unwrap_or("horizon.emote_wheel.empty"), &[]),
            (_, Some(i)) => Text::literal(&names[i]),
            _ => Text::translate("horizon.emote_wheel.title", &[]),
        };
        let plain = title.plain();
        let tw = HudBatch::text_width(atlas, &plain);
        batch.text_styled(
            atlas,
            &title,
            cx - tw / 2.0,
            cy - OUTER * scale - 20.0,
            u32::MAX,
            true,
        );
        // The centre is the wheel's dead zone: a visible Back/Cancel target
        // there works even for an empty wheel, without covering any sector.
        let cancel = [
            cx - 28.0 * scale,
            cy - 10.0 * scale,
            56.0 * scale,
            20.0 * scale,
        ];
        horizon_client::ui::action_button(
            batch,
            atlas,
            horizon_client::widgets::Action::Back,
            cancel[0],
            cancel[1],
            cancel[2],
            cancel[3],
            &Text::translate("gui.cancel", &[]),
            true,
            horizon_client::ui::hovered(self.cursor, cancel[0], cancel[1], cancel[2], cancel[3]),
        );
        let help_key = if n > 0 {
            load_status.unwrap_or("horizon.emote_wheel.help")
        } else {
            "horizon.emote_wheel.help"
        };
        let help = Text::translate(help_key, &[]);
        let hw = HudBatch::text_width(atlas, &help.plain());
        batch.text_styled(
            atlas,
            &help,
            cx - hw / 2.0,
            cy + OUTER * scale + 10.0,
            0xFFA0_A0A8,
            true,
        );
    }
}

// Isolated actual-Game fixture: dummy UUIDs and already-completed producer
// channels, with no authentication or service calls. No world tick is invoked.
impl Game<'_> {
    pub(in crate::game) fn emote_catalogue_fixture(
        &mut self,
    ) -> horizon_client::Result<serde_json::Value> {
        use horizon_cosmetics::account::OwnedEmotes;
        use std::sync::mpsc;
        let item = || {
            horizon_cosmetics::Item::parse(br#"{"format":1,"id":"wave","slot":"emote","parts":[],"emote":{"length":20,"tracks":[{"part":"head","keys":[{"t":0}]}]}}"#).unwrap()
        };
        let completed = |answer| {
            let (tx, rx) = mpsc::channel();
            tx.send(Ok(answer)).unwrap();
            rx
        };
        let now = Instant::now();
        self.screen = Screen::Paused;
        self.open_emote_wheel();
        assert!(self.emote_wheel_pauses());
        let ages: Vec<_> = self.mobs.iter().map(|m| m.age).collect();
        self.apply_owned_emotes(Some(101), now);
        self.cosmetics.emotes.catalogue.loading = Some(completed(OwnedEmotes {
            emotes: vec![(item(), Default::default())],
            complete: true,
        }));
        // This is the same completion application called by cosmetics_frame and
        // wheel refresh, using an injected owner snapshot instead of credentials.
        self.apply_owned_emotes(Some(101), now);
        self.draw_emote_wheel(&mut HudBatch::new([480, 320]));
        assert_eq!(self.cosmetics.wheel.displayed, ["wave"]);
        assert!(self.emote_wheel_pauses());
        assert_eq!(self.mobs.iter().map(|m| m.age).collect::<Vec<_>>(), ages);
        let loaded = serde_json::json!({"case":"paused_success","displayed":self.cosmetics.wheel.displayed,"paused":true,"worldAgesUnchanged":true});

        self.cosmetics.emotes.catalogue.loading = Some(completed(OwnedEmotes {
            emotes: vec![],
            complete: false,
        }));
        self.apply_owned_emotes(Some(101), now);
        self.draw_emote_wheel(&mut HudBatch::new([480, 320]));
        assert_eq!(self.cosmetics.wheel.displayed, ["wave"]);
        assert!(!self.cosmetics.emotes.catalogue.due(now));
        assert!(
            self.cosmetics
                .emotes
                .catalogue
                .due(now + Duration::from_secs(15))
        );
        assert_eq!(
            self.cosmetics.emotes.catalogue.status(),
            Some("horizon.emote_wheel.retry")
        );
        let incomplete = serde_json::json!({"case":"incomplete_retains_good","displayed":self.cosmetics.wheel.displayed,"retryAfter15s":true,"status":self.cosmetics.emotes.catalogue.status()});

        let (tx, rx) = mpsc::channel();
        self.cosmetics.emotes.catalogue.loading = Some(rx);
        self.apply_owned_emotes(Some(202), now);
        assert!(
            tx.send(Ok(OwnedEmotes {
                emotes: vec![(item(), Default::default())],
                complete: true
            }))
            .is_err()
        );
        self.draw_emote_wheel(&mut HudBatch::new([480, 320]));
        assert!(self.cosmetics.wheel.displayed.is_empty());
        assert!(self.cosmetics.emotes.names.is_empty());
        let changed = serde_json::json!({"case":"owner_change","displayed":self.cosmetics.wheel.displayed,"staleResultDiscarded":true});

        self.cosmetics.emotes.catalogue.loading = Some(completed(OwnedEmotes {
            emotes: vec![],
            complete: true,
        }));
        self.apply_owned_emotes(Some(202), now);
        self.draw_emote_wheel(&mut HudBatch::new([480, 320]));
        assert!(self.cosmetics.wheel.displayed.is_empty());
        assert!(
            !self
                .cosmetics
                .emotes
                .catalogue
                .due(now + Duration::from_secs(599))
        );
        assert_eq!(self.cosmetics.emotes.catalogue.status(), None);
        let empty = serde_json::json!({"case":"complete_empty","displayed":self.cosmetics.wheel.displayed,"freshFor600s":true});
        self.apply_owned_emotes(None, now);
        assert!(
            !self
                .cosmetics
                .emotes
                .catalogue
                .due(now + Duration::from_secs(9999))
        );
        Ok(serde_json::json!([loaded, incomplete, changed, empty]))
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
