//! A notice about a controller nobody has to answer: translated words, the controller's own glyph
//! if it helps, how long it stays. A view holds one at a time (`NoticeSlot`), so they never stack:
//! each player's game has its own slot, and what raises a notice (splitscreen's joins and
//! reconnects, a solo game's controller coming and going) decides that for itself.
use crate::gamepad::{Button, GlyphStyle};
use crate::hud::{HudAtlas, HudBatch, rgba};
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

/// A button shown before the body, in the player's glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub button: Button,
    pub style: GlyphStyle,
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
    /// The notice under `top` on a view: a strip in the middle, its words wrapped to the view (a
    /// toast is as wide as its longest line: off a narrow view's edge), sliding down from the
    /// view's edge.
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
        let glyph = notice.glyph.map(|g| {
            (
                g.button.label(g.style),
                width(g.button.label(g.style)) + 4.0,
            )
        });
        let indent = glyph.map_or(0.0, |g| g.1);
        let lines = crate::split_profiles::wrap(&notice.body, max - indent, width);
        let inner = lines
            .iter()
            .map(|l| width(l) + indent)
            .fold(width(&notice.title), f32::max);
        let (pw, ph) = (inner.min(max) + 16.0, 20.0 + 10.0 * lines.len() as f32);
        let px = ((w - pw) / 2.0).floor();
        let py = (top - (1.0 - notice.slide.portion) * (top + ph)).floor();
        panel(batch, atlas, [px, py, pw, ph]);
        let white = rgba(255, 255, 255, 255);
        let tx = (px + (pw - width(&notice.title)) / 2.0).floor();
        batch.text(atlas, &notice.title, tx, py + 6.0, white, true);
        for (k, line) in lines.iter().enumerate() {
            let lx = (px + (pw - width(line) - indent) / 2.0).floor();
            let y = py + 18.0 + 10.0 * k as f32;
            if let (0, Some((label, _))) = (k, glyph) {
                batch.text(atlas, label, lx, y, rgba(255, 255, 255, 255), false);
            }
            batch.text(atlas, line, lx + indent, y, rgba(200, 200, 200, 255), false);
        }
    }
}

/// The prompts' and notices' box: dark, a faint edge.
pub fn panel(batch: &mut HudBatch, atlas: &HudAtlas, [px, py, pw, ph]: [f32; 4]) {
    batch.fill(atlas, px, py, pw, ph, rgba(0, 0, 0, 210));
    let edge = rgba(255, 255, 255, 90);
    for [x, y, fw, fh] in [
        [px, py, pw, 1.0],
        [px, py + ph - 1.0, pw, 1.0],
        [px, py, 1.0, ph],
        [px + pw - 1.0, py, 1.0, ph],
    ] {
        batch.fill(atlas, x, y, fw, fh, edge);
    }
}

/// A button as printed on the pad, in its colour, on a dark key `gw` wide.
pub fn key(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    label: &str,
    color: u32,
    x: f32,
    y: f32,
    gw: f32,
) {
    batch.fill(atlas, x - 1.0, y - 2.0, gw, 11.0, rgba(40, 40, 40, 255));
    batch.text(atlas, label, x + 1.0, y, color, false);
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
