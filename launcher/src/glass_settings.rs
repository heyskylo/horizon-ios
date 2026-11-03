//! The Settings page in the glass look: the mockup's `SettingsView`. A list of the sections on
//! the left, and the sections as glass cards in a pane that scrolls on its own under the title;
//! the list follows the scroll and takes the pane to a section. The rows inside are the mockup's
//! `SettingRow`s ([`glass_ui::flat_rows`]).
//!
//! Not in the mockup, kept in its patterns: the appearance, the Home background, the logs folder and the applications menu, the AI
//! clients, data collection (its own entry in the list) and the hidden developer options.

use super::*;
use crate::glass_ui::{self as ui, white};
use horizon_theme::glass::{fill, ink, radius};

/// The list's entries: name and icon. The mockup's seven, then data collection.
const SECTIONS: [(&str, &str); 8] = [
    ("Account", "user"),
    ("Updates", "refresh"),
    ("Launcher", "computer"),
    ("Network", "globe-02"),
    ("Game", "game-controller-01"),
    ("AI", "ai-chat-02"),
    ("About", "information-circle"),
    ("Data collection", "view"),
];

/// What `settings_page` builds before it knows which look draws it.
pub(super) struct Built {
    pub install: Stateful<Div>,
    pub prune: Stateful<Div>,
    pub opening: Stateful<Div>,
    pub menu_entry: Stateful<Div>,
    pub appearance: AnyElement,
    pub horizon: String,
    pub horizon_action: Option<AnyElement>,
    pub launcher: String,
    pub launcher_action: Option<AnyElement>,
}

/// A section's card (`hzl-glass rounded-[20px] p-2`).
fn card() -> Div {
    crate::glass::plate(radius::PANEL)
        .flex()
        .flex_col()
        .p(px(8.))
}

fn note(s: &str) -> Option<AnyElement> {
    Some(div().child(tr(s).to_string()).into_any_element())
}

impl Launcher {
    pub(super) fn glass_settings(&mut self, b: Built, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let sections = ui::flat_rows(|| self.glass_settings_sections(b, cx));
        let active = self.settings_section(sections.len());
        let nav =
            card()
                .flex_none()
                .w(px(210.))
                .gap(px(4.))
                .children(SECTIONS.iter().enumerate().map(|(i, &(label, icon))| {
                    let on = i == active;
                    keyboard(
                        div()
                            .id(SharedString::from(format!("settings-nav-{i}")))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .h(px(40.))
                            .px(px(12.))
                            .rounded(px(radius::ROW))
                            .cursor_pointer()
                            .font_family(horizon_theme::glass::FONT)
                            .text_size(px(14.))
                            .when(on, |d| {
                                d.bg(white(fill::ACTIVE))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(white(ink::FULL))
                            })
                            .when(!on, |d| {
                                d.text_color(white(0.65))
                                    .hover(|s| s.bg(white(0.05)).text_color(white(ink::FULL)))
                            })
                            .child(ui::icon(
                                icon,
                                17.,
                                if on {
                                    ui::color(ui::look().accent_soft)
                                } else {
                                    white(0.65)
                                },
                            ))
                            .child(tr(label).to_string()),
                        &t,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.settings_pane.scroll_to_top_of_item(i);
                            cx.notify();
                        },
                    ))
                }));
        let pane = div()
            .id("settings-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_pane)
            .rounded(px(radius::PANEL))
            .flex()
            .flex_col()
            .gap(px(16.))
            .pb(px(8.))
            .children(sections);
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .child(ui::page_head(
                tr("Settings"),
                tr("Accounts, updates, and the launcher itself."),
            ))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .flex_1()
                    .min_h_0()
                    .child(nav)
                    .child(pane),
            )
    }

    /// The section the list marks: the last one whose top is in the pane's upper 30 %, the
    /// last of all once the pane is at its end (as the mockup's scroll spy).
    fn settings_section(&self, count: usize) -> usize {
        let pane = self.settings_pane.bounds();
        let max = self.settings_pane.max_offset().y;
        if max > px(0.) && -self.settings_pane.offset().y >= max - px(2.) {
            return count - 1;
        }
        let mut current = 0;
        for i in 0..count {
            match self.settings_pane.bounds_for_item(i) {
                Some(b) if b.top() - pane.top() <= pane.size.height * 0.3 => current = i,
                _ => break,
            }
        }
        current
    }

    /// The cards, one per entry of [`SECTIONS`], in its order.
    fn glass_settings_sections(&mut self, b: Built, cx: &mut Context<Self>) -> Vec<Div> {
        let t = self.theme;
        let n = 0;
        let account = card().child(self.glass_account(cx));
        let updates = card()
            .child(index_row(
                0,
                tr("Install new versions automatically"),
                note("Your selected version only changes if you were playing the newest one."),
                Some(b.install.into_any_element()),
                &t,
            ))
            .child(index_row(
                0,
                tr("Delete old versions automatically"),
                Some(
                    div()
                        .child(trf(
                            "Keeps the {count} newest, the one in use and the ones your instances use. Deleted versions install again from the Versions page.",
                            &[("count", &self.settings.keep_newest().to_string())],
                        ))
                        .into_any_element(),
                ),
                Some(b.prune.into_any_element()),
                &t,
            ));
        let launcher = card()
            .child(index_row(
                0,
                tr("Appearance"),
                note("Dark by default. System follows your computer's light or dark."),
                Some(b.appearance),
                &t,
            ))
            .child(self.accent_row(cx))
            .child(self.home_background_row(cx))
            .child(self.language_row(cx))
            .child(self.while_playing_row(n, cx))
            .child(index_row(
                0,
                tr("Show the opening"),
                note("The wordmark, while the release list loads."),
                Some(b.opening.into_any_element()),
                &t,
            ));
        let network = card().child(self.proxy_row(n, cx));
        let game = card().child(self.game_args_row(n, cx));
        let ai = card()
            .child(self.no_ai_row(n, cx))
            .children(self.agents_section(cx));
        let mono = |s: String| {
            div()
                .font_family(theme::MONO)
                .text_size(px(12.))
                .child(s)
                .into_any_element()
        };
        let about = card()
            .child(index_row(
                0,
                "Horizon",
                Some(div().child(b.horizon).into_any_element()),
                b.horizon_action,
                &t,
            ))
            .child(index_row(
                0,
                tr("Launcher"),
                Some(div().child(b.launcher).into_any_element()),
                b.launcher_action,
                &t,
            ))
            .child(index_row(
                0,
                tr("Data folder"),
                Some(mono(short(&backend::data_dir()))),
                None,
                &t,
            ))
            .child(index_row(
                0,
                tr("Logs"),
                Some(mono(format!(
                    "{}  ·  {}",
                    short(&backend::logs_dir()),
                    tr("crash reports beside it")
                ))),
                Some(
                    button("open-logs", "Open", Kind::Secondary, &t)
                        .on_click(|_, _, cx| {
                            let dir = backend::logs_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            cx.open_with_system(&dir);
                        })
                        .into_any_element(),
                ),
                &t,
            ))
            .when(cfg!(target_os = "linux"), |d| {
                d.child(index_row(
                    0,
                    tr("Applications menu"),
                    note(if self.settings.desktop_entry() {
                        "Horizon and its icon are in your desktop's menu."
                    } else {
                        "Not in your desktop's menu."
                    }),
                    Some(b.menu_entry.into_any_element()),
                    &t,
                ))
            })
            .children(self.dev_options_section(cx));
        let data = card().child(self.collection_section(cx));
        vec![account, updates, launcher, network, game, ai, about, data]
    }
}
