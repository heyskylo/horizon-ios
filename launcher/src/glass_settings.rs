//! The Settings page in the glass look: the mockup's `SettingsView`. A list of the sections on
//! the left, and the sections as glass cards in a pane that scrolls on its own under the title;
//! the list follows the scroll and takes the pane to a section. The rows inside are the mockup's
//! `SettingRow`s ([`glass_ui::flat_rows`]).
//!
//! Not in the mockup, kept in its patterns: the appearance, the Home background, the data and
//! logs folders and the applications menu (About's rows), the AI clients, data collection (its own
//! entry in the list) and the hidden developer options. The mockup's About tiles for Horizon and
//! the launcher open Updates here, beside the switches that decide how updates install. The mockup's Native mods switch is the
//! native mods' build profile here: they are always on.

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

/// A tile (the mockup's `AboutTile`, `rounded-[14px] bg-white/[0.045] p-5`): its lead, name, a
/// line under it and what it does.
fn tile(lead: AnyElement, title: String, note: String, action: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(140.))
        .rounded(px(radius::LARGE))
        .bg(white(0.045))
        .p(px(20.))
        .child(div().child(lead))
        .child(
            ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                .mt(px(16.))
                .child(title),
        )
        .child(
            ui::text(13., FontWeight::NORMAL, 0.6)
                .mt(px(2.))
                .truncate()
                .child(note),
        )
        .child(div().mt(px(16.)).flex().items_start().child(action))
}

/// The scroll spy: which section is being read, given each section's top from the pane's top
/// (`tops`, in order), the pane's `height`, how far it is `scrolled` and how far it can be (`max`).
/// The last section whose top passed a reading line, which sits at 30 % of the pane (the mockup's)
/// and slides down to the pane's bottom over the last 70 % of a pane of scroll: the short last
/// sections, which never reach 30 %, each come in turn as the end nears, instead of the mockup's
/// jump from wherever it was to the last one at the very end.
fn spy(tops: &[f32], height: f32, scrolled: f32, max: f32) -> usize {
    let rest = height * 0.3;
    let travel = height - rest;
    let tail = travel.min(max);
    let line = if tail > 0. {
        rest + travel * ((scrolled - (max - tail)) / tail).clamp(0., 1.)
    } else {
        rest
    };
    tops.iter().rposition(|&top| top <= line + 0.5).unwrap_or(0)
}

fn note(s: &str) -> Option<AnyElement> {
    Some(div().child(tr(s).to_string()).into_any_element())
}

impl Launcher {
    pub(super) fn glass_settings(&mut self, b: Built, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let sections = ui::flat_rows(|| self.glass_settings_sections(b, cx));
        let active = self.settings_section(sections.len());
        self.settings_marked = active;
        let wide = self.window_width >= crate::glass_home::COMPACT;
        let items: Vec<_> = SECTIONS
            .iter()
            .enumerate()
            .map(|(i, &(label, icon))| {
                let on = i == active;
                keyboard(
                    div()
                        .id(SharedString::from(format!("settings-nav-{i}")))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(if wide { 12. } else { 8. }))
                        .h(px(if wide { 40. } else { 34. }))
                        .px(px(if wide { 12. } else { 10. }))
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
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.settings_pane.scroll_to_top_of_item(i);
                    cx.notify();
                }))
            })
            .collect();
        let nav = if wide {
            card()
                .flex_none()
                .w(px(210.))
                .gap(px(4.))
                .children(items)
                .into_any_element()
        } else {
            crate::glass::plate(radius::PANEL)
                .id("settings-nav-strip")
                .flex_none()
                .w_full()
                .flex_row()
                .overflow_x_scroll()
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .gap(px(4.))
                .p(px(6.))
                .children(items)
                .into_any_element()
        };
        let pane = div()
            .id("settings-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_pane)
            // A scroll redraws the pane from its layer without rendering the page again, so the
            // list follows it here: on the next frame, once the offset has moved, and only when
            // the section changed.
            .on_scroll(cx.listener(|_, _: &gpui::ScrollEvent, window, cx| {
                cx.on_next_frame(window, |this, _, cx| {
                    if this.settings_section(SECTIONS.len()) != this.settings_marked {
                        cx.notify();
                    }
                });
            }))
            .rounded(px(radius::PANEL))
            .flex()
            .flex_col()
            .gap(px(16.))
            .pb(px(8.))
            .children(sections);
        let content = if wide {
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(12.))
                .flex_1()
                .min_h_0()
                .child(nav)
                .child(pane)
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .flex_1()
                .min_h_0()
                .child(nav)
                .child(pane)
        };
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .child(ui::page_head(
                tr("Settings"),
                tr("Accounts, updates, and the launcher itself."),
            ))
            .child(content)
    }

    /// The section the list marks, from where the pane's sections are now ([`spy`]). A section's
    /// bounds are where layout put it, before the scroll (`ScrollHandle` keeps the offset apart):
    /// the scroll is taken off them here. Reading them as they were left the list on the first
    /// section until the pane hit its end.
    fn settings_section(&self, count: usize) -> usize {
        let pane = self.settings_pane.bounds();
        let scrolled = -self.settings_pane.offset().y;
        let tops: Vec<f32> = (0..count)
            .map_while(|i| self.settings_pane.bounds_for_item(i))
            .map(|b| f32::from(b.top() - pane.top() - scrolled))
            .collect();
        spy(
            &tops,
            f32::from(pane.size.height),
            f32::from(scrolled),
            f32::from(self.settings_pane.max_offset().y),
        )
    }

    /// The Game card's switch for registry mods' automatic updates (`horizon::auto`).
    fn mods_auto_update_row(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let on = self.settings.auto_updates_mods();
        ui::setting_row(
            tr("Update registry mods automatically"),
            Some(
                div()
                    .child(tr(
                        "At start and before you play, what you installed from Horizon's registry moves to its newest version for this game. Never inside a modpack, and an update that would remove blocks or items waits for you.",
                    ))
                    .into_any_element(),
            ),
            Some(
                crate::switch("mods-auto-update", on, &t)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings.mods_auto_update = Some(!this.settings.auto_updates_mods());
                        this.save_settings();
                        cx.notify();
                    }))
                    .into_any_element(),
            ),
        )
    }

    /// The Game card's Native mods row. The mockup has an on/off switch; native mods are always
    /// on here, so the row picks the profile they build with, as the Mods page does.
    fn native_mods_row(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let profile = native_mods::Profile::chosen(&self.settings);
        let picker = segments(
            "settings-native-profile",
            &[
                (native_mods::Profile::Performance, "Performance build"),
                (native_mods::Profile::Dev, "Dev build"),
                (native_mods::Profile::Release, "Release build"),
            ],
            profile,
            &t,
            {
                let this = cx.entity().downgrade();
                move |choice, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.settings.native_profile = Some(choice);
                        this.settings.native_release = false;
                        this.native_done = None;
                        this.save_settings();
                        cx.notify();
                    });
                }
            },
        );
        let said = match profile {
            native_mods::Profile::Performance => tr("The everyday build for playing with mods"),
            native_mods::Profile::Dev => tr(
                "For developing a mod: about 6× faster to rebuild a mod edit, game logic up to about twice as slow",
            ),
            native_mods::Profile::Release => {
                tr("Release build: slower to build, slightly faster game")
            }
        };
        ui::setting_row(
            tr("Native mods"),
            Some(
                div()
                    .child(format!(
                        "{} {said}.",
                        tr("Experimental. Builds the native mods you trust into a game of their own.")
                    ))
                    .into_any_element(),
            ),
            Some(picker.into_any_element()),
        )
    }

    /// Updates' two tiles (the mockup's `AboutTile`s): Horizon and its update, the launcher and its
    /// own.
    fn update_tiles(
        &mut self,
        horizon: String,
        horizon_action: Option<AnyElement>,
        launcher: String,
        launcher_action: Option<AnyElement>,
    ) -> Div {
        let installed = self
            .settings
            .selected
            .clone()
            .map(|tag| match &self.releases {
                Load::Ready(list) => list
                    .iter()
                    .find(|r| r.tag == tag)
                    .map_or(tag.clone(), |r| update_name(r).to_owned()),
                _ => tag,
            });
        let tone = if self.updates.horizon.is_some() {
            ui::Tone::Accent(ui::look().accent_soft)
        } else if self.update_error.is_some() {
            ui::Tone::Warn
        } else if self.checked.is_some() {
            ui::Tone::Live
        } else {
            ui::Tone::Busy
        };
        let ink = match tone {
            ui::Tone::Live => ui::live(),
            ui::Tone::Warn => ui::warn(),
            _ => white(0.7),
        };
        let status = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(13.))
            .text_color(ink)
            .child(ui::dot(tone))
            .child(div().min_w_0().truncate().child(horizon));
        let launcher_mark = div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(42.))
            .rounded(px(11.))
            .bg(white(0.08))
            .child(ui::icon("computer", 21., white(0.8)))
            .into_any_element();
        let wide = self.window_width >= crate::glass_home::SIDE_BY_SIDE;
        div()
            .flex()
            .when(wide, |d| d.flex_row())
            .when(!wide, |d| d.flex_col())
            .gap(px(8.))
            .child(tile(
                self.horizon_mark(42.),
                "Horizon".into(),
                installed.unwrap_or_else(|| tr("Nightly").to_string()),
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(12.))
                    .child(status)
                    .children(horizon_action)
                    .into_any_element(),
            ))
            .child(tile(
                launcher_mark,
                tr("Launcher").to_string(),
                launcher,
                div().children(launcher_action).into_any_element(),
            ))
    }

    /// The cards, one per entry of [`SECTIONS`], in its order.
    fn glass_settings_sections(&mut self, b: Built, cx: &mut Context<Self>) -> Vec<Div> {
        let t = self.theme;
        let n = 0;
        let account = card().child(self.glass_account(cx));
        let updates = card()
            .child(self.update_tiles(b.horizon, b.horizon_action, b.launcher, b.launcher_action))
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
            .child(self.launcher_theme_row(cx))
            .child(self.glass_accent_row(cx))
            .child(self.look_slider_row(themes_page::LookKnob::Glass, cx))
            .child(self.look_slider_row(themes_page::LookKnob::Blur, cx))
            .child(self.performance_row(cx))
            .child(self.home_background_row(cx))
            .child(self.language_row(cx))
            .child(self.while_playing_row(n, cx))
            .child(index_row(
                0,
                tr("Show the opening"),
                note("The wordmark, while the release list loads."),
                Some(b.opening.into_any_element()),
                &t,
            ))
            .child(self.reset_look_row(cx));
        let network = card().child(self.proxy_row(n, cx));
        let game = card()
            .child(self.game_architecture_row(n, cx))
            .child(self.game_args_row(n + 1, cx))
            .child(self.mods_auto_update_row(cx))
            .child(self.native_mods_row(cx));
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
            .child(self.location_row(0, cx).mb(px(0.)))
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
                            crate::glass_toast::open(cx, &dir);
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

#[cfg(test)]
mod tests {
    use super::spy;

    /// A 600 px pane over the mockup's eight cards: tall ones, then About and Data collection,
    /// too short for their tops ever to reach 30 % of the pane.
    const TOPS: [f32; 8] = [0., 420., 700., 1000., 1300., 1600., 1900., 2100.];
    const HEIGHT: f32 = 600.;
    const MAX: f32 = 2300. - HEIGHT;

    fn at(scrolled: f32) -> usize {
        let tops: Vec<f32> = TOPS.iter().map(|t| t - scrolled).collect();
        spy(&tops, HEIGHT, scrolled, MAX)
    }

    #[test]
    fn the_list_follows_the_section_read_at_30_percent() {
        assert_eq!(at(0.), 0);
        assert_eq!(
            at(300.),
            1,
            "Updates' top is 120 px down: in the upper 30 %"
        );
        assert_eq!(at(600.), 2);
        assert_eq!(
            at(1000.),
            3,
            "Network's top at the pane's top, Game's 300 px down"
        );
        assert_eq!(at(1150.), 4, "Game's top 150 px down: in the upper 30 %");
    }

    #[test]
    fn near_the_end_each_short_section_comes_in_turn() {
        let seen: Vec<usize> = (0..=MAX as usize)
            .step_by(10)
            .map(|s| at(s as f32))
            .collect();
        // Every section is marked on the way down, in order, never skipped.
        let mut order = seen.clone();
        order.dedup();
        assert_eq!(order, (0..8).collect::<Vec<_>>());
        // About is marked for a stretch of scroll before the end, not for one step.
        let about = seen.iter().filter(|&&s| s == 6).count();
        assert!(about >= 8, "About marked for {about} steps of 10 px");
        assert_eq!(at(MAX), 7);
    }

    #[test]
    fn a_pane_that_does_not_scroll_marks_the_first() {
        assert_eq!(spy(&[0., 200.], 600., 0., 0.), 0);
        assert_eq!(spy(&[], 600., 0., 0.), 0);
    }
}
