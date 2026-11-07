//! The Versions page in the glass look: the mockup's `VersionsView`. Each Nightly on its own glass
//! card (its name, Latest on the first, when it came out and which instances play it), Install
//! or a bin on the right, and Install new versions automatically by the page's title.
//!
//! Not in the mockup, kept in its patterns: Use and In use (the version instances without their
//! own play), the space versions take, Import a game build, the native and imported builds,
//! install progress and errors, the confirmation before a removal.

use super::*;
use crate::glass_ui::{self as ui, Tone};
use horizon_theme::glass::{ink, radius};

/// A release's day as the mockup writes it ("Nov 1"), with its time in UTC when known.
pub(super) fn published(date: &str, time: &str) -> String {
    let day = date
        .get(8..10)
        .map(|d| d.trim_start_matches('0').to_string())
        .unwrap_or_default();
    let args = [("day", day.as_str())];
    let day = match date.get(5..7) {
        Some("01") => trf("Jan {day}", &args),
        Some("02") => trf("Feb {day}", &args),
        Some("03") => trf("Mar {day}", &args),
        Some("04") => trf("Apr {day}", &args),
        Some("05") => trf("May {day}", &args),
        Some("06") => trf("Jun {day}", &args),
        Some("07") => trf("Jul {day}", &args),
        Some("08") => trf("Aug {day}", &args),
        Some("09") => trf("Sep {day}", &args),
        Some("10") => trf("Oct {day}", &args),
        Some("11") => trf("Nov {day}", &args),
        Some("12") => trf("Dec {day}", &args),
        _ => date.to_string(),
    };
    if time.is_empty() {
        day
    } else {
        trf("{day}, {time} UTC", &[("day", &day), ("time", time)])
    }
}

/// A version's state beside its name: a dot and a word, 13 px at 70 % (`Latest`).
fn badge(tone: Tone, label: &str) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .child(ui::dot(tone))
        .child(ui::text(13., FontWeight::NORMAL, ink::BODY).child(tr(label).to_string()))
}

/// The compact title and commit line for a published Nightly.
///
/// The release title from GitHub is an implementation-facing ISO date and commit. Players need
/// the date first, with the short commit kept as the secondary detail so the newest build is easy
/// to recognize at a glance.
fn nightly_card_text(release: &Release) -> Option<(String, String)> {
    release.nightly.as_ref()?;
    let date = published(&release.date, "");
    let title = format!("{} · {date}", tr("Nightly"));
    let commit = horizon_launcher::nightly::label(&release.tag)
        .and_then(|label| {
            label
                .split_once(" · ")
                .map(|(_, commit)| commit.chars().take(7).collect())
        })
        .unwrap_or_default();
    Some((title, commit))
}

impl Launcher {
    /// The glass Versions page; `on_disk` and `used` as `versions_page` measured them.
    pub(super) fn glass_versions(
        &self,
        on_disk: &dyn Fn(&String) -> Option<u64>,
        used: u64,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let auto = switch("versions-auto-install", self.settings.auto_install(), &t).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.settings.install_new_versions = Some(!this.settings.auto_install());
                this.save_settings();
                if this.settings.auto_install() {
                    this.install_update();
                }
                cx.notify();
            }),
        );
        let wide = self.window_width >= crate::glass_home::SIDE_BY_SIDE;
        let page_title = ui::page_head(
            tr("Versions"),
            tr(
                "Horizon's Nightly builds, newest first. Your worlds live in the game \
                folder, so removing a version never touches them.",
            ),
        )
        .mb_0();
        let auto_switch = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .child(
                ui::text(14., FontWeight::NORMAL, 0.85)
                    .child(tr("Install new versions automatically")),
            )
            .child(auto);
        let head = if wide {
            div()
                .flex()
                .flex_row()
                .items_end()
                .justify_between()
                .gap(px(20.))
                .child(page_title)
                .child(auto_switch)
                .mb(px(28.))
        } else {
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(16.))
                .child(page_title.w_full())
                .child(auto_switch)
                .mb(px(28.))
        };
        // The space they take and Import a game build (not in the mockup): a quiet line above
        // the list.
        let import: AnyElement = if self.importing_build {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .w(px(180.))
                .child(progress_bar(None, &t))
                .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(tr("Importing…")))
                .into_any_element()
        } else {
            ui::quiet("import-build", tr("Import a game build…"), 36., &t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.import_build(cx)))
                .into_any_element()
        };
        let sponsor = (!self.links.sponsor.is_empty()).then(|| {
            let url = self.links.sponsor.clone();
            crate::glass::plate(radius::PANEL)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .p(px(24.))
                .mb(px(24.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            ui::text(16., FontWeight::SEMIBOLD, ink::FULL)
                                .child(tr("Support the work below")),
                        )
                        .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(tr(
                            "Horizon is built in the open. Sponsors fund the next release.",
                        ))),
                )
                .child(
                    ui::quiet_icon("sponsor", Some("github"), tr("Sponsor on GitHub"), 36., &t)
                        .on_click(move |_: &ClickEvent, _, cx| cx.open_url(&url)),
                )
        });
        let mut page = div().flex().flex_col().child(head).children(sponsor).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .mb(px(10.))
                .child(
                    ui::text(13., FontWeight::NORMAL, ink::QUIET).when(used > 0, |d| {
                        d.child(trf(
                            "Installed versions use {size}.",
                            &[("size", &megabytes(used))],
                        ))
                    }),
                )
                .child(import),
        );
        page = page
            .children(self.import_error.clone().map(|e| notice(e, &t).mb(px(10.))))
            .children(self.glass_local_builds(
                "Native mod builds",
                native_mods::is_native,
                None,
                on_disk,
                cx,
            ))
            .children(self.glass_local_builds(
                "Imported builds",
                build_import::is_imported,
                Some(tr(
                    "Unsigned: you picked these programs yourself, and Horizon can't \
                    check where they came from. Updates never replace them.",
                )),
                on_disk,
                cx,
            ));
        let releases = match &self.releases {
            Load::Loading => return page.child(centered(tr("Loading releases…"), &t)),
            Load::Failed(e) => {
                return page.child(
                    centered(&trf("Couldn't load releases. {error}", &[("error", e)]), &t).child(
                        button("retry", "Try again", Kind::Secondary, &t).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.fetch_releases();
                                cx.notify();
                            },
                        )),
                    ),
                )
            }
            Load::Ready(r) if r.is_empty() => {
                return page.child(centered(tr("No releases yet."), &t));
            }
            Load::Ready(r) => r,
        };
        let keep = |i: usize| {
            let tag = &releases[i].tag;
            self.installing.as_ref().is_some_and(|(t, _)| t == tag)
                || self.install_error.as_ref().is_some_and(|(t, _)| t == tag)
                || self.confirm_remove.as_ref() == Some(tag)
                || self.removing.contains(tag)
        };
        let list = self.long_list.clone().column(
            &self.content_scroll.handle,
            releases.len(),
            10.,
            keep,
            |i| {
                let release = releases[i].clone();
                let (title, mut line) = nightly_card_text(&release).unwrap_or_else(|| {
                    let title = release
                        .name
                        .split_once(": ")
                        .map(|(_, rest)| rest.to_string())
                        .unwrap_or_else(|| release.name.clone());
                    let title = if title.is_empty() {
                        release.tag.clone()
                    } else {
                        title
                    };
                    (title, published(&release.date, &release.time))
                });
                if line.is_empty() {
                    line = published(&release.date, &release.time);
                }
                let right = self.glass_release_action(&release, cx);
                let notes = self.notes_section(&release, cx);
                let card = self
                    .glass_version_card(&release.tag, title, &mut line, i == 0, on_disk, right)
                    .child(notes);
                if i == 1 {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            ui::text(13., FontWeight::NORMAL, ink::QUIET)
                                .child(tr("Older versions kept")),
                        )
                        .child(card)
                } else {
                    card
                }
            },
        );
        page.child(list)
    }

    /// One version's card: its name (Latest on the first, In use on the one in use), `line`
    /// with who plays it, and `right`.
    fn glass_version_card(
        &self,
        tag: &str,
        title: String,
        line: &mut String,
        latest: bool,
        on_disk: &dyn Fn(&String) -> Option<u64>,
        right: AnyElement,
    ) -> Div {
        let t = self.theme;
        let look = ui::look();
        let installed = self.installed.iter().any(|v| v == tag);
        let in_use = self.selected().map(String::as_str) == Some(tag);
        let players: Vec<&str> = self
            .instances
            .list
            .iter()
            .filter(|i| i.java.is_none() && self.version_of(i).map(String::as_str) == Some(tag))
            .map(|i| i.name.as_str())
            .collect();
        let mut add = |part: String| {
            if !line.is_empty() {
                line.push_str(" · ");
            }
            line.push_str(&part);
        };
        if !players.is_empty() {
            add(trf("played by {names}", &[("names", &players.join(", "))]));
        } else if installed {
            add(tr("installed").to_string());
        }
        if let Some(n) = on_disk(&tag.to_string()) {
            add(trf("{size} on disk", &[("size", &megabytes(n))]));
        }
        let error = self
            .install_error
            .as_ref()
            .filter(|(e, _)| e == tag)
            .map(|(_, e)| e.clone());
        crate::glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        ui::text(16., FontWeight::SEMIBOLD, ink::FULL)
                                            .line_height(px(24.))
                                            .truncate()
                                            .child(title),
                                    )
                                    .when(latest, |d| {
                                        d.child(badge(Tone::Accent(look.accent_soft), "Latest"))
                                    })
                                    .when(in_use, |d| d.child(badge(Tone::Live, "In use"))),
                            )
                            .child(
                                ui::text(13., FontWeight::NORMAL, 0.6)
                                    .line_height(px(19.5))
                                    .truncate()
                                    .child(line.clone()),
                            ),
                    )
                    .child(div().relative().flex().flex_none().child(right)),
            )
            .children(error.map(|e| notice(e, &t)))
    }

    /// What a release's card offers on its right: its install's progress, Install, or Use and
    /// the bin once installed.
    fn glass_release_action(&self, release: &Release, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let tag = release.tag.clone();
        if let Some((_, p)) = self.installing.as_ref().filter(|(t, _)| *t == tag) {
            return self
                .glass_progress(Some(*p), tr("Installing"))
                .into_any_element();
        }
        if self.removing.contains(&tag) {
            return self
                .glass_progress(None, tr("Removing…"))
                .into_any_element();
        }
        if self.installed.contains(&tag) {
            if backend::needs_game_architecture_install(&tag, release) {
                let b = ui::quiet(
                    SharedString::from(format!("install-selected-architecture-{tag}")),
                    tr("Install selected build"),
                    36.,
                    &t,
                );
                if self.installing.is_some() {
                    return b.opacity(0.4).cursor_default().into_any_element();
                }
                let release = release.clone();
                return b
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.install(&release);
                        cx.notify();
                    }))
                    .into_any_element();
            }
            return self.glass_version_actions(&tag, cx).into_any_element();
        }
        if release.asset.is_none() {
            return ui::text(13., FontWeight::NORMAL, ink::QUIET)
                .child(tr("No build for this platform"))
                .into_any_element();
        }
        let b = ui::quiet(
            SharedString::from(format!("install-{tag}")),
            tr("Install"),
            36.,
            &t,
        );
        if self.installing.is_some() {
            b.opacity(0.4).cursor_default().into_any_element()
        } else {
            let release = release.clone();
            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.install(&release);
                cx.notify();
            }))
            .into_any_element()
        }
    }

    /// A bar with its word, 180 px: an install's progress, or a removal's.
    fn glass_progress(&self, fraction: Option<f32>, label: &str) -> Div {
        let t = self.theme;
        let word = match fraction {
            Some(p) => format!("{:.0}%", p * 100.),
            None => label.to_string(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .w(px(180.))
            .child(progress_bar(fraction.map(|_| self.bar), &t))
            .child(
                ui::text(13., FontWeight::NORMAL, ink::QUIET)
                    .flex_none()
                    .child(word),
            )
    }

    /// An installed version's actions: Use (when it isn't the one in use) and the bin, which asks
    /// again before it removes.
    fn glass_version_actions(&self, tag: &str, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let selected = self.selected().map(String::as_str) == Some(tag);
        let confirming = self.confirm_remove.as_deref() == Some(tag);
        let (t1, t2, t3) = (tag.to_string(), tag.to_string(), tag.to_string());
        let mut d = div().flex().items_center().gap(px(8.));
        if confirming {
            return d
                .child(
                    button(format!("remove-cancel-{tag}"), "Cancel", Kind::Ghost, &t).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirm_remove = None;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ui::quiet(
                        SharedString::from(format!("remove-{tag}")),
                        tr("Remove for good"),
                        36.,
                        &t,
                    )
                    .text_color(ui::danger())
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.remove(&t2);
                            cx.notify();
                        },
                    )),
                );
        }
        if !selected {
            d = d.child(
                ui::quiet(SharedString::from(format!("use-{tag}")), tr("Use"), 36., &t).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.settings.selected = Some(t1.clone());
                        this.save_settings();
                        cx.notify();
                    }),
                ),
            );
        }
        d.child(
            ui::icon_button(
                SharedString::from(format!("bin-{tag}")),
                "delete-02",
                36.,
                17.,
                radius::CONTROL,
                false,
                &t,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.confirm_remove = Some(t3.clone());
                cx.notify();
            })),
        )
    }

    /// The installed builds of one kind made here, as cards under a small heading; None when
    /// there are none.
    fn glass_local_builds(
        &self,
        title: &str,
        is: fn(&str) -> bool,
        notice_: Option<&str>,
        on_disk: &dyn Fn(&String) -> Option<u64>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let tags: Vec<String> = self
            .installed
            .iter()
            .chain(&self.removing)
            .filter(|v| is(v))
            .cloned()
            .collect();
        (!tags.is_empty()).then(|| {
            let mut d = div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .mb(px(24.))
                .child(ui::card_heading(tr(title).to_string()))
                .children(notice_.map(|n| notice(n.to_string(), &self.theme)));
            for tag in tags {
                let mut line = Vec::new();
                if let Some(about) = native_mods::about(&tag) {
                    line.push(trf("Built on {base}", &[("base", &about.base)]));
                }
                if let Some(about) = build_import::about(&tag) {
                    let base = horizon_launcher::nightly::label(&about.base).unwrap_or(about.base);
                    line.push(trf("Game files from {base}", &[("base", &base)]));
                    line.push(about.file);
                }
                let mut line = line.join(" · ");
                let right = if self.removing.contains(&tag) {
                    self.glass_progress(None, tr("Removing…"))
                        .into_any_element()
                } else {
                    self.glass_version_actions(&tag, cx).into_any_element()
                };
                let title = horizon_launcher::nightly::label(&tag)
                    .or_else(|| build_import::label(&tag))
                    .unwrap_or_else(|| tag.trim_start_matches('v').to_string());
                d = d.child(self.glass_version_card(&tag, title, &mut line, false, on_disk, right));
            }
            d
        })
    }
}

impl Launcher {
    /// A release's "Patch notes" toggle and, once opened, its notes under it (the release's
    /// own Markdown). Opened state is kept per release in `notes_open`.
    pub(super) fn notes_section(&self, release: &Release, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let open = self.notes_open.contains(&release.tag);
        let tag = release.tag.clone();
        let id = SharedString::from(format!("notes-{tag}"));
        let turn = |s: gpui::Svg| {
            s.with_transformation(gpui::Transformation::rotate(gpui::radians(if open {
                std::f32::consts::PI
            } else {
                0.
            })))
        };
        let (label, chevron) = if t.glass {
            (
                ui::text(14., FontWeight::MEDIUM, ink::BODY).child(tr("Patch notes")),
                turn(ui::icon("arrow-down-01", 16., ui::white(ink::NOTE))),
            )
        } else {
            (
                text(theme::UI)
                    .text_color(t.ink_soft)
                    .child(tr("Patch notes")),
                turn(blocks::icon("chevron-down", 16., t.muted)),
            )
        };
        let toggle = div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .child(label)
            .child(chevron)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.notes_open.remove(&tag) {
                    this.notes_open.insert(tag.clone());
                }
                cx.notify();
            }));
        let body = open.then(|| {
            div()
                .max_w(px(640.))
                .child(if release.body.trim().is_empty() {
                    text(theme::UI)
                        .text_color(t.muted)
                        .child(tr("No notes for this release."))
                        .into_any_element()
                } else {
                    markdown::render(&release.body, &t)
                })
        });
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(toggle)
            .children(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nightly_cards_put_plain_date_before_commit() {
        let release = Release {
            tag: "nightly-2026-11-07-4aa6854".into(),
            name: "Nightly 2026-11-07 · 4aa6854".into(),
            body: String::new(),
            date: "2026-11-07".into(),
            time: "14:32".into(),
            asset: None,
            prerelease: true,
            nightly: Some("nightly".into()),
        };

        let (title, commit) = nightly_card_text(&release).expect("nightlies have card text");
        assert!(title.contains("Nightly"));
        assert!(title.contains("Nov 7"));
        assert_eq!(commit, "4aa6854");
    }
}
