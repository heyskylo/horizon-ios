//! Home's tabbed panel in the glass look (the mockup's `TabbedPanel` and `Overview`,
//! `launcher-views.tsx`): one glass plate whose top-left corner is the tab strip, and under it
//! the open tab. Overview shows the instance's last screenshots beside the newest patch log,
//! then what Play still needs (the launcher's rows). Dark appearance only.
use super::*;
use crate::glass_ui::{self as ui, white};
use crate::scrollbar::TakeWheel;
use home_tabs::HomeTab;
use horizon_theme::glass::{ink, radius};

/// The tabbed panel's own body is at least this tall (`min-h-[340px]`), and so is the patch log.
const BODY: f32 = 340.;
/// The game log's lines in Overview: what its 200 px show (16 px around, 21 px a line).
const OVERVIEW_LOG: usize = 8;

impl Launcher {
    /// The tabbed panel: the strip, then the open tab. `rows`: what Overview shows under its
    /// screenshots and patch log (what Play needs, the open export or launch panel).
    pub(super) fn glass_home_tabs(&mut self, rows: Div, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let tabs = self.home_tab_list(cx);
        let open = tabs
            .iter()
            .map(|(tab, _)| *tab)
            .find(|tab| *tab == self.home_tab)
            .unwrap_or(HomeTab::Overview);
        let compact = self.window_width < crate::glass_home::SIDE_BY_SIDE;
        let very_compact = self.window_width < crate::glass_home::NARROW;
        let super_compact = self.window_width < crate::glass_home::NARROWEST;
        let pad = if super_compact {
            4.
        } else if very_compact {
            6.
        } else if compact {
            8.
        } else {
            14.
        };
        let gap = if very_compact {
            1.
        } else if compact {
            2.
        } else {
            4.
        };
        let strip = div()
            .id("home-tabs-strip")
            .relative()
            .flex()
            .items_center()
            .gap(px(gap))
            .px(px(8.))
            .pt(px(8.))
            .pb(px(4.))
            // At its own size the strip leaves room for `glass::tabbed`'s notch; on a narrow
            // window it takes the panel's width and its tabs wrap onto a further row, so every
            // tab stays in reach from the panel's left padding and nothing scrolls sideways.
            .when(!compact, |d| d.flex_none())
            .when(compact, |d| d.flex_wrap().w_full().min_w_0())
            .child(crate::glass::measure(self.home_strip.clone()))
            .children(tabs.into_iter().map(|(tab, count)| {
                let (name, icon) = match tab {
                    HomeTab::Overview => {
                        (tr("Overview"), (!compact).then_some("dashboard-square-01"))
                    }
                    HomeTab::Mods => (tr("Mods"), (!compact).then_some("puzzle")),
                    HomeTab::Worlds => (tr("Worlds"), (!compact).then_some("earth")),
                    HomeTab::Multiplayer => {
                        (tr("Multiplayer"), (!compact).then_some("user-multiple-02"))
                    }
                    HomeTab::Packs => (tr("Packs"), None),
                    HomeTab::Screenshots => (tr("Screenshots"), None),
                    HomeTab::Console => {
                        (tr("Console"), (!compact).then_some("computer-terminal-01"))
                    }
                };
                let mut item = ui::count_tab(
                    format!("home-tab-{tab:?}"),
                    icon,
                    name,
                    count.filter(|n| *n > 0),
                    tab == open,
                    &t,
                );
                if compact {
                    item = item.px(px(pad));
                }
                item.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.home_tab = tab;
                    cx.notify();
                }))
            }));
        let body: Option<AnyElement> = match open {
            HomeTab::Overview => Some(self.glass_overview(rows, cx).into_any_element()),
            HomeTab::Mods => Some(self.glass_instance_mods(cx).into_any_element()),
            HomeTab::Worlds => self.glass_worlds(cx).map(IntoElement::into_any_element),
            HomeTab::Multiplayer => self
                .glass_multiplayer(cx)
                .map(IntoElement::into_any_element),
            HomeTab::Packs => self.glass_packs(cx).map(IntoElement::into_any_element),
            HomeTab::Screenshots => self
                .glass_screenshots(cx)
                .map(IntoElement::into_any_element),
            HomeTab::Console => self.glass_console(cx).map(IntoElement::into_any_element),
        };
        div()
            .relative()
            // `mt-12` under the head, of which Home's own gap gives 28.
            .mt(px(20.))
            .child(crate::glass::tabbed(self.home_strip.clone()))
            .child(
                div()
                    .relative()
                    .flex()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(strip),
            )
            .child(
                div().relative().p(px(8.)).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .min_h(px(BODY))
                        .px(px(4.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .children(body),
                ),
            )
    }

    /// Overview: the last screenshots and the patch log side by side, then `rows`.
    fn glass_overview(&mut self, rows: Div, cx: &mut Context<Self>) -> Div {
        let wide = self.window_width >= crate::glass_home::SIDE_BY_SIDE;
        let shots = self.overview_shots(cx);
        let patch = div()
            .relative()
            .when(wide, |d| d.flex_none().w(px(380.)))
            .when(!wide, |d| d.w_full())
            .min_h(px(BODY))
            .child(self.patch_log(cx));
        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .children(self.overview_log())
            .child(
                div()
                    .flex()
                    .when(wide, |d| d.flex_row().gap(px(20.)))
                    .when(!wide, |d| d.flex_col().gap(px(20.)))
                    .child(shots)
                    .child(patch),
            )
            .child(rows)
    }

    /// The game log while this instance prepares, starts or runs (the mockup's, over the
    /// screenshots): its newest lines, as many as `max-h-[200px]` shows scrolled to the end.
    fn overview_log(&self) -> Option<Div> {
        let instance = self.selected_instance()?;
        let mine = match (&self.client, &self.starting_client) {
            (Some(_), _) => self.client_home.as_ref() == Some(&instance.home),
            (None, Some(starting)) => *starting == instance.id || self.selected() == Some(starting),
            (None, None) => self.preparing,
        };
        if !mine || self.prepare_log.is_empty() {
            return None;
        }
        let count = self.prepare_log.len();
        let shown = self
            .prepare_log
            .iter()
            .skip(count.saturating_sub(OVERVIEW_LOG));
        Some(
            div()
                .flex()
                .flex_col()
                .px(px(12.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .child(card_title(tr("Game log")))
                        .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(
                            if count == 1 {
                                tr("1 line").to_string()
                            } else {
                                trf("{count} lines", &[("count", &count.to_string())])
                            },
                        )),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .p(px(16.))
                        .rounded(px(radius::ROW))
                        .bg(gpui::black().opacity(0.25))
                        .font_family(theme::MONO)
                        .text_size(px(12.))
                        .line_height(px(21.))
                        .text_color(white(0.75))
                        .children(shown.map(|line| div().truncate().child(line.clone()))),
                ),
        )
    }

    /// The instance's three newest screenshots (the first twice as big when there are three),
    /// "See all" opening the Screenshots tab; a line about F2 while there are none.
    fn overview_shots(&mut self, cx: &mut Context<Self>) -> Div {
        let count = self.screenshots_count();
        let shots: Vec<_> = self.screenshots.list.iter().take(3).cloned().collect();
        let pictures: Vec<AnyElement> = shots
            .iter()
            .map(|s| self.shot_picture(&s.path, cx))
            .collect();
        let privacy = self.shots_privacy(cx);
        let see_all = || {
            cx.listener(
                |this: &mut Self, _: &ClickEvent, _, cx: &mut Context<Self>| {
                    this.home_tab = HomeTab::Screenshots;
                    cx.notify();
                },
            )
        };
        let head = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .h(px(32.))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(card_title(tr("Screenshots")))
                    .children((count > 0).then(|| {
                        ui::text(16., FontWeight::NORMAL, ink::DIM).child(count.to_string())
                    })),
            )
            .children((count > 0).then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(privacy)
                    .child(
                        ui::text(13., FontWeight::NORMAL, ink::NOTE)
                            .id("shots-see-all")
                            .cursor_pointer()
                            .hover(|s| s.text_color(white(ink::FULL)))
                            .child(tr("See all"))
                            .on_click(see_all()),
                    )
            }));
        let mut pictures = pictures.into_iter();
        let mut shot = |i: usize| {
            div()
                .id(SharedString::from(format!("overview-shot-{i}")))
                .overflow_hidden()
                .rounded(px(radius::ROW))
                .cursor_pointer()
                .children(pictures.next())
        };
        let body = match shots.len() {
            0 => ui::text(14., FontWeight::NORMAL, ink::NOTE)
                .mt(px(8.))
                .child(tr("No screenshots yet. Press F2 in the game."))
                .into_any_element(),
            n if n >= 3 => {
                let first = shot(0).flex_1().h_full().on_click(see_all());
                let rest: Vec<_> = (1..n)
                    .map(|i| shot(i).flex_1().w_full().on_click(see_all()))
                    .collect();
                div()
                    .mt(px(12.))
                    .flex()
                    .gap(px(8.))
                    .w_full()
                    .aspect_ratio(8. / 3.)
                    .child(first)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .w(gpui::relative(1. / 3.))
                            .flex_none()
                            .h_full()
                            .children(rest),
                    )
                    .into_any_element()
            }
            // `grid-cols-2`: one picture takes half the width.
            n => div()
                .mt(px(12.))
                .grid()
                .grid_cols(2)
                .gap(px(8.))
                .children(
                    (0..n).map(|i| shot(i).w_full().aspect_ratio(16. / 9.).on_click(see_all())),
                )
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .px(px(12.))
            .child(head)
            .child(body)
    }

    /// The patch log (`PatchLog`): a Horizon instance's Nightly changes by section (the newest
    /// Nightly's when it plays another version), else the newest launcher release's; "All
    /// notes" opening Patch notes. It fills its column and scrolls inside.
    fn patch_log(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let horizon = self.selected_instance().is_some_and(|i| i.java.is_none());
        let played = horizon.then(|| self.selected().cloned()).flatten();
        let (note, sections) = match &self.releases {
            Load::Ready(list) => patch_source(list, played.as_deref()),
            _ => None,
        }
        .unwrap_or_else(|| (tr("No notes yet").to_string(), Vec::new()));
        let many = sections.len() > 1;
        let list =
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .mt(px(16.))
                .children(sections.into_iter().map(|(section, items)| {
                    div()
                        .flex()
                        .flex_col()
                        .children(
                            many.then(|| {
                                ui::text(13., FontWeight::MEDIUM, ink::QUIET).child(section)
                            }),
                        )
                        .child(div().mt(px(6.)).flex().flex_col().gap(px(6.)).children(
                            items.into_iter().map(|item| {
                                div()
                                    .flex()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex_none()
                                            .mt(px(9.))
                                            .size(px(5.))
                                            .rounded_full()
                                            .bg(white(0.4)),
                                    )
                                    .child(
                                        ui::text(14., FontWeight::NORMAL, ink::STRONG)
                                            .line_height(px(21.))
                                            .min_w_0()
                                            .child(item),
                                    )
                            }),
                        ))
                }));
        div()
            .id("patch-log")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .p(px(16.))
            .rounded(px(radius::LARGE))
            .bg(white(0.04))
            .overflow_y_scroll()
            .track_scroll(&self.patch_scroll.handle)
            // The list takes the wheel while it can still move that way; at an end, or when
            // everything fits, the page behind it gets it.
            .take_wheel(&self.patch_scroll)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(12.))
                    .child(card_title(tr("Patch log")))
                    .child(
                        ui::text(13., FontWeight::NORMAL, ink::NOTE)
                            .id("patch-log-all")
                            .cursor_pointer()
                            .hover(|s| s.text_color(white(ink::FULL)))
                            .child(tr("All notes"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.go(Page::Versions);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                ui::text(13., FontWeight::NORMAL, ink::NOTE)
                    .mt(px(2.))
                    .child(note),
            )
            .child(list)
    }
}

/// A release's changes: each section's name and its lines.
type Sections = Vec<(String, Vec<String>)>;

/// What the patch log shows from `releases` and its line under the title: the release `played`
/// installs as when it lists changes, else the newest Nightly that does ("Nightly 2026-11-01 ·
/// 4aa6854 · Nov 1"); for a Java instance (`played` None) or without one, the newest launcher
/// release ("Launcher 0.2.37 · Nov 2").
fn patch_source(releases: &[Release], played: Option<&str>) -> Option<(String, Sections)> {
    let day = |r: &Release| glass_versions::published(&r.date, "");
    if let Some(played) = played {
        let nightly = releases
            .iter()
            .filter(|r| r.tag == played)
            .chain(releases.iter().filter(|r| r.nightly.is_some()))
            .map(|r| (r, patch_sections(&r.body)))
            .find(|(_, sections)| !sections.is_empty());
        if let Some((r, sections)) = nightly {
            return Some((format!("{} · {}", r.name, day(r)), sections));
        }
    }
    let r = releases
        .iter()
        .find(|r| r.nightly.is_none() && !r.prerelease)?;
    let version = r
        .tag
        .rsplit('@')
        .next()
        .unwrap_or(&r.tag)
        .trim_start_matches('v');
    let note = format!(
        "{} · {}",
        trf("Launcher {version}", &[("version", version)]),
        day(r)
    );
    Some((note, patch_sections(&r.body)))
}

/// A card's title (`CARD_TITLE`): 16 px semibold.
fn card_title(label: impl Into<SharedString>) -> Div {
    ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(label.into())
}

/// A release's changes by section ("### New", "### Fixed"), each line without its pull request
/// numbers; the contributors' thanks and what follows the notes are left out.
fn patch_sections(body: &str) -> Sections {
    let mut out: Sections = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("### ") {
            out.push((name.trim().to_string(), Vec::new()));
        } else if line == "---" || line.starts_with("Your launcher updates itself") {
            break;
        } else if let (Some(item), Some((section, items))) =
            (line.strip_prefix("- "), out.last_mut())
        {
            if section.as_str() != "Contributors" {
                items.push(strip_refs(item));
            }
        }
    }
    out.retain(|(name, items)| name != "Contributors" && !items.is_empty());
    out
}

/// "Fix the picker (#6389, fixes #6342)" → "Fix the picker".
fn strip_refs(item: &str) -> String {
    let mut s = item.trim();
    while let Some(open) = s.rfind(" (") {
        let tail = &s[open..];
        if tail.ends_with(')') && tail.contains('#') {
            s = s[..open].trim_end();
        } else {
            break;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_body_becomes_its_sections_without_refs_or_thanks() {
        let body = "### New\n- Native mods: export a headless engine (#6462)\n\n### Fixed\n- Play with friends: any port (#6363, fixes #6342)\n- Keep (this) aside\n\n### Contributors\nThanks to @a.\n\nYour launcher updates itself. New here?\n\n---\nNot an official Minecraft product.";
        assert_eq!(
            patch_sections(body),
            vec![
                (
                    "New".to_string(),
                    vec!["Native mods: export a headless engine".to_string()]
                ),
                (
                    "Fixed".to_string(),
                    vec![
                        "Play with friends: any port".to_string(),
                        "Keep (this) aside".to_string()
                    ]
                ),
            ]
        );
    }

    fn release(tag: &str, name: &str, date: &str, nightly: bool, body: &str) -> Release {
        Release {
            tag: tag.into(),
            name: name.into(),
            body: body.into(),
            date: date.into(),
            time: String::new(),
            asset: None,
            prerelease: false,
            nightly: nightly.then(|| "nightly".to_string()),
        }
    }

    #[test]
    fn the_patch_log_shows_the_played_nightly_else_the_newest_else_the_launcher() {
        let releases = [
            release(
                "nightly-2026-11-03-2944480",
                "Nightly 2026-11-03 · 2944480",
                "2026-11-03",
                true,
                "### Fixed\n- Newest fix (#2)",
            ),
            release(
                "nightly-2026-11-01-4aa6854",
                "Nightly 2026-11-01 · 4aa6854",
                "2026-11-01",
                true,
                "### New\n- Older change (#1)",
            ),
            release(
                "nightly-2026-10-30-0c02067",
                "Nightly 2026-10-30 · 0c02067",
                "2026-10-30",
                true,
                "No list.",
            ),
            release(
                "launcher@0.2.37",
                "Horizon launcher 0.2.37",
                "2026-11-02",
                false,
                "### Fixed\n- Launcher fix",
            ),
        ];
        let (note, sections) = patch_source(&releases, Some("nightly-2026-11-01-4aa6854")).unwrap();
        assert_eq!(note, "Nightly 2026-11-01 · 4aa6854 · Nov 1");
        assert_eq!(
            sections,
            vec![("New".to_string(), vec!["Older change".to_string()])]
        );
        // A version that lists no changes: the newest Nightly's.
        let (note, _) = patch_source(&releases, Some("nightly-2026-10-30-0c02067")).unwrap();
        assert_eq!(note, "Nightly 2026-11-03 · 2944480 · Nov 3");
        // A Java instance: the launcher's.
        let (note, sections) = patch_source(&releases, None).unwrap();
        assert_eq!(note, "Launcher 0.2.37 · Nov 2");
        assert_eq!(
            sections,
            vec![("Fixed".to_string(), vec!["Launcher fix".to_string()])]
        );
    }
}
