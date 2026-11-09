//! What splitscreen draws over a player's view: its badge, questions answered with the pad and
//! notices.
use super::*;
use horizon_client::controller_notice::NoticeSlot;
use horizon_client::gamepad::{GlyphStyle, MenuJob};
use horizon_client::hud::HudAtlas;
use horizon_client::hud::{HudBatch, rgba};
use horizon_client::hud_kit::{
    fit::ellipsize,
    hint::{self, HintRow},
    plate::{self, Plate},
    room::Room,
};
use horizon_client::pad_glyph::{Glyph, Glyphs};
use std::time::Duration;

/// Where the join strip and notices go with nothing above them, in GUI px from the view's top.
const TOP: f32 = 18.0;
/// A prompt's height in GUI px: its title and a row of glyphs.
const STRIP: f32 = 34.0;
/// The gap in GUI px between the join strip and a notice under it.
const GAP: f32 = 4.0;
/// GUI px a prompt's plate keeps clear of the view's sides, and its text of the plate's.
const SIDE: f32 = 12.0;
const INNER: f32 = 8.0;
/// GUI px after an answer's words, before the next answer's glyph.
const BETWEEN: f32 = 12.0;

/// A prompt's answers as hint rows, each caption cut to an equal share of what `max` (GUI px)
/// leaves after the glyphs and the gaps, when the whole line is wider than that.
fn answers(
    atlas: &HudAtlas,
    choices: &[(MenuJob, String)],
    places: Glyphs,
    max: f32,
) -> Vec<HintRow> {
    let rows: Vec<HintRow> = choices
        .iter()
        .map(|(job, what)| HintRow::new([Glyph::menu(*job, places)], what.as_str()))
        .collect();
    if line_width(atlas, &rows) <= max {
        return rows;
    }
    let captions: f32 = rows.iter().map(|r| atlas.font.width(&r.caption)).sum();
    let share = (max - (line_width(atlas, &rows) - captions)) / rows.len().max(1) as f32;
    rows.iter()
        .map(|r| HintRow {
            glyphs: r.glyphs.clone(),
            caption: ellipsize(atlas, &r.caption, share).into_owned(),
        })
        .collect()
}

/// How wide a prompt's answers draw on one line: each row, then `BETWEEN` after its words.
fn line_width(atlas: &HudAtlas, rows: &[HintRow]) -> f32 {
    let drawn: f32 = rows.iter().map(|r| advance(atlas, r) + after(r)).sum();
    drawn - BETWEEN
}

/// The room after an answer: `BETWEEN` after words; a glyph alone sits by the next.
fn after(row: &HintRow) -> f32 {
    if row.caption.is_empty() { 0.0 } else { BETWEEN }
}

/// From one answer's glyph to where the next one's goes, less the gap after its words: its
/// glyphs, the gap, its words. The glyph's art starts a pixel left of where it draws
/// (`pad_glyph::rect`), so the gap from there is a pixel less.
fn advance(atlas: &HudAtlas, row: &HintRow) -> f32 {
    let look = &hint::PROMPT;
    row.glyphs_width(atlas, look) + look.gap - 1.0 + atlas.font.width(&row.caption)
}

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
    /// Each answer: the menu job whose button gives it, and its words.
    pub(super) choices: Vec<(MenuJob, String)>,
    /// The family of the controller asked (a joining one), whose glyphs answer it; none when any
    /// controller may answer (one lost), shown in this player's family.
    pub(super) family: Option<GlyphStyle>,
}

/// What a prompt asks: a controller may join (every view, a strip at the top: nobody stops
/// playing for it), or its controller disconnected (the view dimmed beneath: any controller
/// answers).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Ask {
    Join,
    Lost,
}

/// Where the join strip and a notice go on a view, as `(strip, notice)` tops in GUI px: at
/// `TOP`, or below the boss bars when they reach past it (`Room::below_top`); the notice under
/// the strip while it shows (`below_strip`).
fn tops(room: &Room, below_strip: bool) -> (f32, f32) {
    let strip = room.below_top(TOP);
    let notice = if below_strip {
        strip + STRIP + GAP
    } else {
        strip
    };
    (strip, notice)
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
            (MenuJob::Confirm, tr("lost.use_another", &[])),
            (MenuJob::Back, tr("lost.leave", &[])),
        ],
        family: None,
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
        let below_strip = matches!(&self.split_hud.prompt, Some(p) if p.ask == Ask::Join);
        let (top, notice_top) = tops(&self.ui.room, below_strip);
        // A question: centred over the view dimmed, or a strip at the top (a controller joining).
        if let Some(prompt) = &self.split_hud.prompt {
            // Join and lost-controller questions are answered by a controller nobody holds yet,
            // whose letters aren't read yet: by places (docs/controllers.md, Nintendo pads).
            let family = prompt.family.unwrap_or(self.pad.style);
            let places = Glyphs::unheld(&self.settings.controller, family);
            let dim = prompt.ask != Ask::Join;
            if dim {
                batch.fill(atlas, 0.0, 0.0, w, h, rgba(0, 0, 0, 120));
            }
            let max = w - 2.0 * SIDE - 2.0 * INNER;
            let rows = answers(atlas, &prompt.choices, places, max);
            let line = line_width(atlas, &rows);
            let title = ellipsize(atlas, &prompt.title, max);
            let pw = width(&title).max(line) + 2.0 * INNER;
            let (px, py) = (
                ((w - pw) / 2.0).floor(),
                if dim {
                    ((h - STRIP) / 2.0).floor()
                } else {
                    top
                },
            );
            plate::draw(batch, atlas, Plate::Notice, [px, py, pw, STRIP]);
            let tx = (px + (pw - width(&title)) / 2.0).floor();
            batch.text(atlas, &title, tx, py + 6.0, rgba(255, 255, 255, 255), true);
            let mut x = (px + (pw - line) / 2.0).floor();
            let y = py + 20.0;
            for row in &rows {
                // `x` is where the glyph draws; its art, and the row, start a pixel left.
                hint::draw_hint(batch, atlas, row, &hint::PROMPT, x - 1.0, y);
                x += advance(atlas, row) + after(row);
            }
        }
        // A notice nobody answers: at the top, under a joining controller's strip, for as long
        // as a toast shows (5 s, times the setting).
        let shown_for =
            Duration::from_secs_f64(5.0 * f64::from(self.settings.notification_display_time));
        self.split_hud
            .notice
            .draw(batch, atlas, notice_top, shown_for);
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

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::hud_kit::fit::ELLIPSIS;
    use horizon_client::hud_kit::room::Keep;
    use horizon_client::hud_overlays::boss_bars_bottom;

    /// A room on a 500 x 300 view with `n` boss bars.
    fn with_bars(n: usize) -> Room {
        let bottom = boss_bars_bottom(300, n) as f32;
        Room::new(500.0, 300.0).with(Keep::BossBars, [159.0, 0.0, 182.0, bottom])
    }

    fn choices() -> Vec<(MenuJob, String)> {
        vec![
            (MenuJob::Confirm, "Join as a new player".into()),
            (MenuJob::Extra, "Play as P1 instead".into()),
            (MenuJob::Back, "Not now".into()),
        ]
    }

    #[test]
    fn a_prompts_answers_fit_a_narrow_view_and_keep_their_glyphs() {
        let atlas = HudAtlas::build(std::path::Path::new("/nonexistent")).unwrap();
        let places = Glyphs::default();
        let whole = answers(&atlas, &choices(), places, f32::INFINITY);
        let wide = line_width(&atlas, &whole);
        // Room enough: every answer whole.
        assert_eq!(answers(&atlas, &choices(), places, wide), whole);
        // A narrow view: room for each answer's glyph and about "Not..." of its words.
        let captions: f32 = whole.iter().map(|r| atlas.font.width(&r.caption)).sum();
        let max = wide - captions + 3.0 * atlas.font.width("Not...");
        let cut = answers(&atlas, &choices(), places, max);
        assert!(
            line_width(&atlas, &cut) <= max,
            "{} in {max}",
            line_width(&atlas, &cut)
        );
        for (row, (job, what)) in cut.iter().zip(choices()) {
            assert_eq!(row.glyphs, [Glyph::menu(job, places)]);
            assert!(row.caption.ends_with(ELLIPSIS) && row.caption.len() < what.len());
        }
    }

    #[test]
    fn with_nothing_showing_the_strip_and_notice_stay_where_they_were() {
        // Before the room: the strip at 18, a notice under it at 56.
        assert_eq!(tops(&Room::new(500.0, 300.0), false), (18.0, 18.0));
        assert_eq!(tops(&Room::new(500.0, 300.0), true), (18.0, 56.0));
    }

    #[test]
    fn the_strip_and_notice_go_below_the_boss_bars_they_would_cover() {
        // One bar ends at 17, above the strip's 18: nothing moves.
        assert_eq!(tops(&with_bars(1), true), (18.0, 56.0));
        // Two end at 36, three at 55: below them, 2 px away.
        assert_eq!(tops(&with_bars(2), false), (38.0, 38.0));
        assert_eq!(tops(&with_bars(3), true), (57.0, 95.0));
    }
}
