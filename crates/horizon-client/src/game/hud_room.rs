//! The HUD's `Room` this frame (`docs/ui-kit.md`): what vanilla's HUD shows that Horizon's own
//! overlays keep clear of. `hud` empties it each frame and `hud_layers` fills it in before the
//! world's HUD draws, into `ui.room`; the controller notice, splitscreen's join strip and the
//! radial menu read it. With it, the frame's `motion::Clock` (`hud_clock`), and whether an
//! overlay holds the view (`overlay_holds_view`), which what vanilla draws there gives way to.
use super::Game;
use horizon_client::hud::HudBatch;
use horizon_client::hud_kit::motion::Clock;
use horizon_client::hud_kit::room::{Keep, Room};
use horizon_client::{boss_overlay, hud_layout, hud_overlays};

impl Game<'_> {
    /// The HUD's clock this frame (`hud_kit::motion`): still, so overlays show their motion
    /// settled, while the game is paused, frame-stepped (`HORIZON_STILL`, a screenshot
    /// sequence) or a repro recording or replay runs.
    pub(super) fn hud_clock(&self) -> Clock {
        let still = self.pause
            || self.clock.still()
            || self.screenshot_sequence.is_some()
            || self.repro.recording_active()
            || self.repro.replaying();
        Clock::new(std::time::Instant::now(), still)
    }

    /// What the HUD shows on a `w` x `h` GUI px view this frame, as keep-outs: the boss bars
    /// while the HUD shows (`show_hud`: not F1, not Replay Studio), as wide as the bar or its
    /// widest name.
    pub(super) fn hud_room(&self, w: f32, h: f32, show_hud: bool) -> Room {
        let mut room = Room::new(w, h);
        if !show_hud {
            return room;
        }
        let (wi, hi) = (w as i32, h as i32);
        let bars = self.boss_overlay.bars(boss_overlay::now_millis());
        let drawn = hud_overlays::boss_bars_drawn(hi, bars.len());
        if drawn > 0 {
            let font = &self.gui.atlas.font;
            let half = bars[..drawn]
                .iter()
                .map(|b| (hud_layout::width(font, &b.name.resolve_legacy()) + 1) / 2)
                .fold(hud_overlays::BOSS_BAR_WIDTH / 2, i32::max);
            let bottom = hud_overlays::boss_bars_bottom(hi, bars.len());
            room.add(
                Keep::BossBars,
                [
                    (wi / 2 - half) as f32,
                    0.0,
                    (2 * half) as f32,
                    bottom as f32,
                ],
            );
        }
        room
    }

    /// The status rows above the hotbar in a `w` x `h` GUI px view, from what they draw this
    /// frame (`draws`): a keep-out from their highest row's top down, as wide as the hotbar. Called
    /// as they are laid out, after `hud_room`, so the overlays drawn over the HUD see them.
    pub(super) fn keep_status_rows(&mut self, draws: &[hud_layout::GuiDraw], w: i32, h: i32) {
        if let Some(top) = hud_layout::draws_top(draws).filter(|top| *top < h) {
            let half = hud_overlays::HOTBAR_HALF_WIDTH;
            self.ui.room.add(
                Keep::StatusRows,
                [
                    (w / 2 - half) as f32,
                    top as f32,
                    (2 * half) as f32,
                    (h - top) as f32,
                ],
            );
        }
    }

    /// An overlay holds the middle of the view (the radial menu, once it shows: a quick tap of
    /// its opener draws nothing, so nothing blinks). The 2D and F3 crosshairs, the chat's lines,
    /// the toasts and the first-person hands give way to it.
    pub(super) fn overlay_holds_view(&self) -> bool {
        self.pad_radial_shown()
    }

    /// ToastManager.extractRenderState: F1 hides the toasts (`hidden`), and so does an overlay
    /// holding the view, as the radial menu's page tabs run across the top where the toasts are.
    /// They go on timing out meanwhile, as under F1.
    pub(super) fn draw_every_toast(&mut self, batch: &mut HudBatch, hidden: bool) {
        let hidden = hidden || self.overlay_holds_view();
        self.draw_toasts(batch, hidden);
        self.draw_tutorial_toasts(batch, hidden);
        self.draw_pack_toasts(batch, hidden);
        self.draw_music_toast(batch, hidden);
    }

    /// The hands drawn in first person this frame: none while an overlay holds the view, as the
    /// radial menu's tiles sit where they are.
    pub(super) fn hands_shown(&self) -> [bool; 2] {
        if self.overlay_holds_view() {
            [false; 2]
        } else {
            self.hands_to_render()
        }
    }
}
