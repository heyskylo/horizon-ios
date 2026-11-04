//! What the launcher just did, said for 4.2 s (the mockup's `say`): on a wide window (1280 px
//! and more) a line in the top bar between the pages and the account, right-aligned at 70 %;
//! narrower, a glass toast along the window's bottom (`hzl-float`, 20 px corners).
//!
//! Any handler can say something, even one with only `&mut App`: the line waits here and the
//! launcher's drain (every 80 ms) draws it, then lets it go.

use super::*;
use crate::glass_ui::{self as ui, white};
use horizon_theme::glass::ink;
use std::path::Path;
use std::sync::Mutex;

/// How long a line stays (`setTimeout(…, 4200)`).
const SHOWN: Duration = Duration::from_millis(4200);

/// The line, when it was said, and its number (a new one restarts the 4.2 s).
static LINE: Mutex<Option<(String, Instant, u64)>> = Mutex::new(None);

/// Says `text` now, in place of whatever was said before.
pub(super) fn say(text: impl Into<String>) {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    let n = line.as_ref().map_or(1, |l| l.2 + 1);
    *line = Some((text.into(), Instant::now(), n));
}

/// The line still showing, and its number.
fn current() -> Option<(String, u64)> {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    if line.as_ref().is_some_and(|l| l.1.elapsed() >= SHOWN) {
        *line = None;
    }
    line.as_ref().map(|l| (l.0.clone(), l.2))
}

/// `/Users/me/x` as `~/x`, as the mockup writes folders.
pub(super) fn tilde(path: &Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

/// Opens `path` in the system's file manager (or its app), and says so.
pub(super) fn open(cx: &mut App, path: &Path) {
    cx.open_with_system(path);
    say(trf("Opened {path}", &[("path", &tilde(path))]));
}

impl Launcher {
    /// From the drain: whether the line changed (a new one, or the last one gone).
    pub(super) fn toast_tick(&mut self) -> bool {
        let now = current().map(|l| l.1);
        let changed = now != self.toast_seen;
        self.toast_seen = now;
        changed
    }

    /// The top bar's line, on a wide window (`hidden … xl:block`).
    pub(super) fn toast_in_topbar(&self) -> Option<Div> {
        (self.window_width >= 1280.).then(|| {
            ui::text(13., FontWeight::NORMAL, 0.7)
                .flex_1()
                .min_w_0()
                .pl(px(16.))
                .text_right()
                .truncate()
                .children(current().map(|l| l.0))
        })
    }

    /// The toast along the bottom, on a narrower window (`xl:hidden`).
    pub(super) fn toast_float(&self) -> Option<Div> {
        if self.window_width >= 1280. {
            return None;
        }
        let (text, n) = current()?;
        Some(
            div()
                .absolute()
                .left(px(16.))
                .right(px(16.))
                .bottom(px(16.))
                .child(
                    crate::glass::float(20.)
                        .px(px(16.))
                        .py(px(12.))
                        .child(
                            ui::text(13., FontWeight::NORMAL, ink::FULL)
                                .relative()
                                .text_color(white(ink::FULL))
                                .child(text),
                        )
                        .with_animation(
                            ElementId::named_usize("toast", n as usize),
                            Animation::new(theme::ENTER).with_easing(ease_out_quint()),
                            |d, p| d.opacity(p).top(px(theme::RISE * (1. - p))),
                        ),
                ),
        )
    }
}
