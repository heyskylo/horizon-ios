//! A notice about a controller nobody has to answer: translated words, the controller's own glyph
//! if it helps, how long it stays. A view holds one at a time (`NoticeSlot`), so they never stack:
//! each player's game has its own slot, and what raises a notice (splitscreen's joins and
//! reconnects, a solo game's controller coming and going) decides that for itself.
use crate::hud::{HudAtlas, HudBatch, rgba};
use crate::hud_kit::fit::{ellipsize, wrap};
use crate::hud_kit::hint::{self, HintRow};
use crate::hud_kit::plate::{self, Plate};
use crate::pad_glyph::{self, Glyph};
use crate::toasts::Slide;
use std::time::{Duration, Instant};

/// What a notice is about: another with the same id takes its place at once, whatever its
/// priority (a controller reconnecting replaces its disconnect) and without sliding in again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeId {
    /// A controller connected or disconnected.
    Connection,
    /// A splitscreen player the server sent away.
    PlayerLeft,
    /// The stick stopped moving the player at its own angle on this server.
    Movement,
}

/// Which of two notices about different things stays up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Good to know; it doesn't replace something that needs the player.
    Info,
    /// The player may have to do something.
    Attention,
}

pub struct Notice {
    id: NoticeId,
    priority: Priority,
    title: String,
    body: String,
    glyph: Option<Glyph>,
    /// When it was raised: it goes that long after, drawn or not (a notice raised on the way
    /// out of a world must not wait for the next one).
    raised: Instant,
    slide: Slide,
}

impl Notice {
    pub fn new(
        id: NoticeId,
        priority: Priority,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id,
            priority,
            title: title.into(),
            body: body.into(),
            glyph: None,
            raised: Instant::now(),
            slide: Slide::default(),
        }
    }
    /// A button shown before the body, in the glyphs of the controller it is about.
    pub fn with_glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }
}

/// The one notice over a player's view.
#[derive(Default)]
pub struct NoticeSlot {
    current: Option<Notice>,
}

impl NoticeSlot {
    /// `notice` shows here: in place of one about the same thing, and of anything that doesn't
    /// outrank it. Otherwise it's dropped, not queued: by the time the other goes it is old news.
    pub fn raise(&mut self, mut notice: Notice) {
        match &mut self.current {
            Some(old) if old.id == notice.id => {
                // Already on screen: the words change and its time starts again.
                notice.slide = std::mem::take(&mut old.slide);
                *old = notice;
            }
            Some(old) if old.priority > notice.priority => {}
            current => *current = Some(notice),
        }
    }
    pub fn clear(&mut self) {
        self.current = None;
    }
    /// What it shows now, for `pad status`.
    pub fn showing(&self) -> Option<(&str, &str)> {
        self.current
            .as_ref()
            .map(|n| (n.title.as_str(), n.body.as_str()))
    }
    /// Its time, after `shown_for`: it slides out (`ToastManager`'s slide); one whose time passed
    /// while nothing drew is simply gone. Then whether anything is left to draw.
    fn update(&mut self, shown_for: Duration) -> bool {
        let Some(notice) = &mut self.current else {
            return false;
        };
        let elapsed = notice.raised.elapsed();
        let finished = if notice.slide.origin.is_none() && elapsed > shown_for {
            true
        } else {
            notice.slide.update_now(|_| elapsed <= shown_for).1
        };
        if finished {
            self.current = None;
        }
        !finished
    }
    /// The notice under `top` (GUI px; the caller keeps it below the boss bars with the HUD's
    /// `Room`) on a view: a strip in the middle, its words wrapped to the view and cut with an
    /// ellipsis where a word or the title is wider than that (a narrow view, a long language),
    /// sliding down from the view's edge.
    pub fn draw(&mut self, batch: &mut HudBatch, atlas: &HudAtlas, top: f32, shown_for: Duration) {
        if !self.update(shown_for) {
            return;
        }
        let Some(notice) = &self.current else {
            return;
        };
        let w = batch.width;
        let width = |text: &str| HudBatch::text_width(atlas, text).ceil();
        let max = (w - 24.0).min(260.0);
        let glyph = notice.glyph.map(|g| (g, pad_glyph::width(atlas, g) + 4.0));
        let indent = glyph.map_or(0.0, |g| g.1);
        let title = ellipsize(atlas, &notice.title, max);
        let lines: Vec<String> = wrap(&notice.body, max - indent, width)
            .iter()
            .map(|line| ellipsize(atlas, line, max - indent).into_owned())
            .collect();
        let inner = lines
            .iter()
            .map(|l| width(l) + indent)
            .fold(width(&title), f32::max);
        let (pw, ph) = (inner.min(max) + 16.0, 20.0 + 10.0 * lines.len() as f32);
        let px = ((w - pw) / 2.0).floor();
        let py = (top - (1.0 - notice.slide.portion) * (top + ph)).floor();
        plate::draw(batch, atlas, Plate::Notice, [px, py, pw, ph]);
        let white = rgba(255, 255, 255, 255);
        let tx = (px + (pw - width(&title)) / 2.0).floor();
        batch.text(atlas, &title, tx, py + 6.0, white, true);
        for (k, line) in lines.iter().enumerate() {
            let lx = (px + (pw - width(line) - indent) / 2.0).floor();
            let y = py + 18.0 + 10.0 * k as f32;
            match (k, glyph) {
                // The glyph's art starts 1 px left of where it is drawn (`pad_glyph::rect`).
                (0, Some((g, _))) => {
                    let row = HintRow::new([g], line.as_str());
                    hint::draw_hint(batch, atlas, &row, &hint::NOTICE, lx - 1.0, y);
                }
                _ => {
                    batch.text(atlas, line, lx + indent, y, hint::NOTICE.color, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(id: NoticeId, priority: Priority, title: &str) -> Notice {
        Notice::new(id, priority, title, "")
    }

    #[test]
    fn a_view_holds_one_notice_however_many_are_raised() {
        let mut slot = NoticeSlot::default();
        for i in 0..50 {
            let title = i.to_string();
            slot.raise(notice(NoticeId::Connection, Priority::Info, &title));
        }
        assert_eq!(slot.showing().map(|s| s.0), Some("49"));
    }

    #[test]
    fn a_reconnect_replaces_its_disconnect_though_it_ranks_lower() {
        let mut slot = NoticeSlot::default();
        slot.raise(notice(NoticeId::Connection, Priority::Attention, "gone"));
        slot.raise(notice(NoticeId::Connection, Priority::Info, "back"));
        assert_eq!(slot.showing().map(|s| s.0), Some("back"));
    }

    #[test]
    fn what_needs_the_player_stays_over_other_news() {
        let mut slot = NoticeSlot::default();
        slot.raise(notice(NoticeId::PlayerLeft, Priority::Attention, "left"));
        slot.raise(notice(NoticeId::Connection, Priority::Info, "back"));
        assert_eq!(slot.showing().map(|s| s.0), Some("left"));
        // News of the same rank is newer.
        slot.raise(notice(NoticeId::Connection, Priority::Attention, "gone"));
        assert_eq!(slot.showing().map(|s| s.0), Some("gone"));
    }

    #[test]
    fn a_notice_raised_while_nothing_draws_is_gone_when_its_time_has_passed() {
        let mut slot = NoticeSlot::default();
        slot.raise(notice(NoticeId::Connection, Priority::Info, "old"));
        std::thread::sleep(Duration::from_millis(2));
        assert!(!slot.update(Duration::ZERO));
        assert_eq!(slot.showing(), None);
    }

    #[test]
    fn a_notice_on_screen_stays_for_its_time_then_slides_out() {
        let mut slot = NoticeSlot::default();
        slot.raise(notice(NoticeId::Connection, Priority::Info, "new"));
        assert!(slot.update(Duration::from_secs(60)));
        std::thread::sleep(Duration::from_millis(2));
        // Past its time it slides out rather than vanishing.
        assert!(slot.update(Duration::ZERO));
        assert!(slot.showing().is_some());
    }
}
