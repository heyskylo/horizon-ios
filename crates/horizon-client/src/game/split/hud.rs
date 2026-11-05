//! What splitscreen draws over a player's view: its badge, questions answered with the pad and
//! notices.
use super::*;
use horizon_client::controller_notice::{NoticeSlot, key, panel};
use horizon_client::gamepad::Button;
use horizon_client::hud::{HudBatch, rgba};
use std::time::Duration;

/// What splitscreen draws over a player's view (`Game::split_hud`): its corner badge (who, for a
/// moment, with why), a question its controller answers, and notices nobody has to answer.
#[derive(Default)]
pub(in crate::game) struct Hud {
    pub(in crate::game) menu: bool,
    pub(in crate::game) badge: Option<String>,
    /// The badge's name stays (P1's Setup, Names); off, it shows only with a place.
    pub(super) names: bool,
    pub(super) place: Option<(String, Instant)>,
    pub(super) prompt: Option<Prompt>,
    pub(in crate::game) notice: NoticeSlot,
}

/// A question on a player's view, answered with its controller's buttons.
#[derive(Clone, PartialEq)]
pub(super) struct Prompt {
    pub(super) ask: Ask,
    pub(super) title: String,
    pub(super) choices: Vec<(Button, String)>,
}

/// What a prompt asks: a controller may join (every view, a strip at the top: nobody stops
/// playing for it), or its controller disconnected (the view dimmed beneath: any controller
/// answers).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Ask {
    Join,
    Lost,
}

/// A splitscreen prompt's text in the selected language (`horizon.splitscreen.prompt.*`).
pub(super) fn tr(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    horizon_client::lang::tr(&format!("horizon.splitscreen.prompt.{key}"), args)
}

/// The question on a waiting player's view.
pub(super) fn lost_prompt(name: &str) -> Prompt {
    Prompt {
        ask: Ask::Lost,
        title: tr("lost", &[&name]),
        choices: vec![
            (Button::South, tr("lost.use_another", &[])),
            (Button::East, tr("lost.leave", &[])),
        ],
    }
}

impl Game<'_> {
    /// P1 opens Setup; another player opens profiles to switch identity.
    pub(in crate::game) fn open_split_settings(&mut self) {
        let page = if self.options.split_guest {
            horizon_client::options_ui::Page::SplitProfiles
        } else {
            horizon_client::options_ui::Page::SplitSetup
        };
        self.open_options_at(false, page);
    }

    /// Splitscreen over this player's view, above the other toasts (`ToastManager`'s slots).
    pub(in crate::game) fn draw_split_hud(&mut self, batch: &mut HudBatch, hidden: bool) {
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        let width = |text: &str| HudBatch::text_width(atlas, text).ceil();
        // After a choice's words, room before the next; a glyph without words ("LB" of "LB RB
        // Move") sits right next to the next glyph.
        let gap = |what: &str| if what.is_empty() { 0.0 } else { 12.0 };
        // The badge, top left: who, and a moment where. F1 hides it with the rest of the HUD.
        if let (false, Some(badge)) = (hidden, &self.split_hud.badge) {
            let place = self.split_hud.place.as_ref();
            let text = match place.filter(|(_, until)| Instant::now() < *until) {
                Some((place, _)) => Some(format!("{badge} · {place}")),
                None => self.split_hud.names.then(|| badge.clone()),
            };
            if let Some(text) = text {
                batch.text(atlas, &text, 2.0, 2.0, rgba(255, 255, 255, 230), true);
            }
        }
        // A question: centred over the view dimmed, or a strip at the top (a controller joining).
        if let Some(prompt) = &self.split_hud.prompt {
            let style = self.ui.options.pad_style;
            let dim = prompt.ask != Ask::Join;
            if dim {
                batch.fill(atlas, 0.0, 0.0, w, h, rgba(0, 0, 0, 120));
            }
            let glyphs: Vec<_> = prompt
                .choices
                .iter()
                .map(|(b, what)| (b.label(style), b.color(style), what.as_str()))
                .collect();
            let line: f32 = glyphs
                .iter()
                .map(|(g, _, what)| width(g) + 8.0 + width(what) + gap(what))
                .sum::<f32>()
                - 12.0;
            let pw = width(&prompt.title).max(line) + 16.0;
            let ph = 34.0;
            let (px, py) = (
                ((w - pw) / 2.0).floor(),
                if dim { ((h - ph) / 2.0).floor() } else { 18.0 },
            );
            panel(batch, atlas, [px, py, pw, ph]);
            let tx = (px + (pw - width(&prompt.title)) / 2.0).floor();
            batch.text(
                atlas,
                &prompt.title,
                tx,
                py + 6.0,
                rgba(255, 255, 255, 255),
                true,
            );
            let mut x = (px + (pw - line) / 2.0).floor();
            let y = py + 20.0;
            for (glyph, [r, g, b], what) in glyphs {
                let gw = width(glyph) + 4.0;
                key(batch, atlas, glyph, rgba(r, g, b, 255), x, y, gw);
                x += gw + 4.0;
                batch.text(atlas, what, x, y, rgba(220, 220, 220, 255), false);
                x += width(what) + gap(what);
            }
        }
        // A notice nobody answers: at the top, under a joining controller's strip, for as long
        // as a toast shows (5 s, times the setting).
        let below_strip = matches!(&self.split_hud.prompt, Some(p) if p.ask == Ask::Join);
        let shown_for =
            Duration::from_secs_f64(5.0 * f64::from(self.settings.notification_display_time));
        let top = if below_strip { 56.0 } else { 18.0 };
        self.split_hud.notice.draw(batch, atlas, top, shown_for);
    }
}

impl<'a> Split<'a> {
    /// The join question on every player's view in the world (a strip at the top), or gone.
    pub(super) fn prompt_join(&mut self, prompt: Option<Prompt>) {
        let views = std::iter::once(&mut self.host.split_hud).chain(
            self.guests
                .iter_mut()
                .filter(|g| g.name.is_some())
                .map(|g| &mut g.game.split_hud),
        );
        for hud in views {
            match &prompt {
                Some(p) => {
                    if hud.prompt.as_ref().is_none_or(|q| q.ask == Ask::Join) {
                        hud.prompt = Some(p.clone());
                    }
                }
                None if hud.prompt.as_ref().is_some_and(|q| q.ask == Ask::Join) => {
                    hud.prompt = None
                }
                None => {}
            }
        }
    }

    /// Player `i`'s badge says `note` for a few seconds.
    pub(super) fn tag(&mut self, i: usize, note: &str) {
        let place = Some((note.to_owned(), Instant::now() + Duration::from_secs(3)));
        if let Some(game) = self.game_mut(i) {
            game.split_hud.place = place;
        }
    }
}
