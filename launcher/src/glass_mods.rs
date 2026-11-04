//! The Mods page in the glass look: the mockup's `ModsView`. The page's tabs on the right of its
//! title, then two glass cards: the search and the list of what it found (icon, name, author,
//! summary, then downloads for the registry's and how it runs for Modrinth's, a tick on what the
//! instance has), and the picked project's card (its
//! icon, name, line, summary, what it may do, downloads, and Install in or Remove from the
//! instance).
//!
//! The mockup's tabs are its sources (Horizon, Modrinth, CurseForge soon); the launcher's page
//! is by kind (mods, modpacks, resource packs, shader packs, data packs), each with its sources,
//! so the tabs are the kinds and the sources are a switch under the search: Horizon's registry
//! or Modrinth, CurseForge beside them as coming soon, on the kinds both serve (`switches`):
//! shader packs are Horizon's registry's only, data packs Modrinth's only, and a Java
//! instance's files always Modrinth's. Modrinth's Java mods are offered to a Horizon instance
//! too, each marked Works, Partly or Doesn't run in Horizon. Not in the mockup, kept in its
//! patterns: Show everything, the filters, which instance or world installs go to (under the
//! search); the
//! project's page, its rating and why it runs or not (in the project's card); adding a native mod
//! from GitHub, the instance's installed list and its native build, imported modpacks and the
//! install's progress (cards under the two).

use super::*;
use crate::glass_ui::{self as ui, white};
use horizon_theme::glass::{ink, radius};

/// The project card's width (`lg:grid-cols-[minmax(0,1fr)_360px]`).
const DETAIL: f32 = 360.;

impl Launcher {
    /// The search's placeholder: the registry's count when it is listed whole.
    pub(super) fn mods_placeholder(&self) -> SharedString {
        let m = &self.mods;
        let registry = m.source == SourceId::Horizon && m.java.is_none();
        if registry && m.kind == PackKind::Mod && m.searched && m.text.text().is_empty() {
            // Results come in pages: only a last page gives the real count.
            if m.more {
                return tr("Search Horizon mods").into();
            }
            trf(
                "Search {count} Horizon mods",
                &[("count", &m.results.len().to_string())],
            )
            .into()
        } else if m.source == SourceId::Modrinth
            && matches!(m.kind, PackKind::ResourcePack | PackKind::Modpack)
        {
            tr("Search resource packs and modpacks").into()
        } else {
            tr("Search, then Enter").into()
        }
    }

    pub(super) fn glass_mods_page(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let instance = self
            .selected_instance()
            .map_or_else(|| "Horizon".to_owned(), |i| i.name.clone());
        let head = ui::page_head(
            tr("Mods"),
            trf(
                "Everything you install goes into {instance}. Pick another instance on the left to fill that one.",
                &[("instance", &instance)],
            ),
        )
        .pt_0()
        .mb_0();
        let open = self.mods.kind;
        let mut tabs = div().flex().flex_none().items_center().gap(px(4.));
        for (kind, label, _) in KINDS {
            tabs = tabs.child(
                small_tab(
                    SharedString::from(format!("mods-tab-{label}")),
                    kind == open,
                    &t,
                )
                .child(tr(label).to_string())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_set_kind(kind);
                    cx.notify();
                })),
            );
        }
        let header = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(20.))
            .pt(px(40. - crate::glass::CONTENT_INSET))
            .mb(px(28.))
            .child(head.min_w_0())
            .child(tabs);

        // A project's own page replaces the two cards, as before.
        if let Some(project) = self.mods_project_page(cx) {
            return div().flex().flex_col().child(header).child(
                crate::glass::plate(radius::PANEL)
                    .p(px(20.))
                    .children(self.mods_busy_plate())
                    .child(project),
            );
        }

        let java = self.mods.java_for(self.mods.kind).is_some();
        let registry = self.mods.source == SourceId::Horizon && !java;
        let everything = self.mods.everything;
        let extras = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(8.))
            .pb(px(8.))
            .when(!registry && !java, |d| {
                d.child(
                    div()
                        .id("mods-everything")
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .child(ui::toggle(
                            "mods-everything-switch",
                            everything,
                            ui::look().accent,
                            ui::look().on_accent,
                            &t,
                        ))
                        .child(ui::text(13., FontWeight::NORMAL, 0.7).child(tr("Show everything")))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.mods.everything = !this.mods.everything;
                            this.mods_search(0);
                            cx.notify();
                        })),
                )
            })
            .when(switches(self.mods.kind) && !java, |d| {
                d.child(self.glass_source_switch(cx))
            })
            .children((!registry).then(|| self.mods_filters(cx)))
            .children(self.mods_target_line(cx));
        // The search, and the sort on its right.
        let search = div()
            .m(px(4.))
            .mb(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(self.mods_field(cx)))
            .child(self.glass_mods_sort(cx));
        let list = crate::glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .p(px(8.))
            .child(search)
            .child(extras)
            .child(self.glass_mods_list(cx));
        // The card stays in view as the list scrolls (`lg:sticky lg:top-0`, `sticky.rs`).
        let detail = self.glass_mod_detail(&instance, cx).map(|d| {
            let down = self.mods_sticky.offset(&self.content_scroll.handle);
            d.flex_none()
                .w(px(DETAIL))
                .top(px(down))
                .child(self.mods_sticky.panel())
        });
        let cards = div()
            .relative()
            .flex()
            .items_start()
            .gap(px(10.))
            .child(self.mods_sticky.row(&self.content_scroll.handle))
            .child(list)
            .children(detail);

        // What the mockup doesn't have, as cards under the two.
        let card = |inner: Div| crate::glass::plate(radius::PANEL).p(px(20.)).child(inner);
        let mut more: Vec<Div> = Vec::new();
        more.extend(self.native_notice(cx).map(card));
        more.extend(self.mods_busy_plate().into_iter().map(card));
        if self.mods.kind == PackKind::Modpack {
            more.push(card(self.imported_packs(cx)));
        }
        if self.mods.kind == PackKind::Mod && self.mods.java.is_none() {
            more.push(card(self.github_panel(cx)));
        }
        more.extend(self.mods_installed_list(cx).map(card));
        div().flex().flex_col().child(header).child(cards).child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .mt(px(10.))
                .children(more),
        )
    }

    /// The source switch (`switches`): Horizon's registry or Modrinth as small tabs, CurseForge
    /// after them as coming soon, shown, inert, never searched, as in the mockup.
    fn glass_source_switch(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let current = self.mods.source;
        let mut row = div().flex().items_center().gap(px(4.));
        for (source, label) in SOURCES {
            row = row.child(
                small_tab(
                    SharedString::from(format!("mods-source-{label}")),
                    source == current,
                    &t,
                )
                .child(tr(label).to_string())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if this.mods.source != source {
                        this.mods.source = source;
                        this.mods.browse.filters = Default::default();
                        this.mods.browse.open = None;
                        this.mods.picked = None;
                        this.mods_search(0);
                    }
                    cx.notify();
                })),
            );
        }
        row.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(32.))
                .px(px(12.))
                .text_size(px(13.))
                .text_color(white(0.35))
                .child("CurseForge")
                .child(tr("soon").to_string()),
        )
    }

    /// The "Sort by" dropdown beside the search: as tall and as round as the search, white at
    /// 7 % like it; the choices on a glass popover under it, the chosen one ticked.
    fn glass_mods_sort(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let sorts = self.mods_sorts();
        let current = self.mods.browse.filters.sort;
        // A sort the source can't do (Follows, picked on Modrinth) reads as the default.
        let shown = sorts
            .iter()
            .find(|(value, _)| *value == current)
            .or(sorts.first())
            .map(|(value, _)| *value);
        let name = sorts
            .iter()
            .find(|(value, _)| Some(*value) == shown)
            .map_or("", |(_, label)| *label);
        let open = self.mods.browse.sort_menu;
        let button = keyboard(
            div()
                .id("mods-sort")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(44.))
                .px(px(14.))
                .rounded(px(12.))
                .bg(white(if open { 0.11 } else { 0.07 }))
                .hover(|s| s.bg(white(0.11)))
                .cursor_pointer()
                .font_family(horizon_theme::glass::FONT)
                .text_size(px(14.))
                .text_color(white(ink::FULL))
                .child(trf("Sort by: {order}", &[("order", tr(name))]))
                .child(
                    ui::icon("arrow-down-01", 16., white(ink::NOTE)).with_transformation(
                        gpui::Transformation::rotate(gpui::radians(if open {
                            std::f32::consts::PI
                        } else {
                            0.
                        })),
                    ),
                )
                .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                    if this.closed_by_this_press(event) {
                        return;
                    }
                    this.mods.browse.sort_menu = !this.mods.browse.sort_menu;
                    cx.notify();
                })),
            &t,
        );
        let mut wrap = div().relative().flex_none().child(button);
        if open {
            let mut menu = ui::popover(220.)
                .id("mods-sort-menu")
                .occlude()
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.mods.browse.sort_menu = false;
                    this.menu_closed_at = Some(event.position);
                    cx.notify();
                }));
            for &(value, label) in &sorts {
                menu = menu.child(
                    ui::pick_row(
                        SharedString::from(format!("mods-sort-pick-{label}")),
                        tr(label).to_string(),
                        Some(value) == shown,
                        &t,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.mods.browse.sort_menu = false;
                            this.mods_filter(|f| f.sort = value);
                            cx.notify();
                        },
                    )),
                );
            }
            wrap = wrap.child(
                gpui::deferred(div().absolute().top(px(52.)).right_0().child(menu))
                    .with_priority(1),
            );
        }
        wrap
    }

    /// The search's results as the mockup's rows (`ROW`): a click picks one for the card beside.
    fn glass_mods_list(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let m = &self.mods;
        if let Some(e) = &m.error {
            return ui::text(14., FontWeight::NORMAL, 0.6)
                .px(px(16.))
                .py(px(24.))
                .child(e.clone());
        }
        if m.results.is_empty() {
            let line = if m.searching || !m.searched {
                tr("Searching…").to_string()
            } else if let Some(target) = m.java_for(m.kind) {
                trf(
                    "Nothing found for {target}.",
                    &[("target", &target.describe(m.kind))],
                )
            } else if !m.text.text().trim().is_empty() {
                trf(
                    "Nothing matches “{query}”.",
                    &[("query", m.text.text().trim())],
                )
            } else if m.source == SourceId::Horizon {
                tr("Nothing in Horizon's registry matches yet.").to_string()
            } else if m.everything {
                tr("Nothing found.").to_string()
            } else {
                tr("Nothing found that runs in Horizon. Show everything lists the rest.")
                    .to_string()
            };
            return ui::text(14., FontWeight::NORMAL, 0.6)
                .px(px(16.))
                .py(px(24.))
                .child(line);
        }
        let picked = self.mods_picked().map(|p| p.id.clone());
        let mut list = div().flex().flex_col();
        for i in 0..self.mods.results.len() {
            let p = self.mods.results[i].clone();
            let icon = self.mods_icon(&p, 40.);
            let m = &self.mods;
            let installed = m.entry(&p).is_some();
            let busy = m.busy.as_ref().is_some_and(|b| b.0 == p.id);
            // The registry's rows end with downloads, the others' with how they run (`compat`).
            let right = if p.source == SourceId::Horizon {
                ui::text(13., FontWeight::NORMAL, 0.5).child(count(p.downloads).to_uppercase())
            } else {
                let support = m.support(&p);
                let tone = match support {
                    Support::Works => ui::live(),
                    Support::Partial(_) => ui::warn(),
                    Support::No(_) => white(0.4),
                };
                ui::text(13., FontWeight::MEDIUM, ink::FULL)
                    .text_color(tone)
                    .child(tr(support.label()))
            };
            let id = p.id.clone();
            list = list.child(
                ui::row(
                    SharedString::from(format!("mods-row-{}", p.id)),
                    picked.as_deref() == Some(p.id.as_str()),
                    12.,
                    &t,
                )
                .px(px(12.))
                .py(px(10.))
                .child(
                    div()
                        .flex_none()
                        .size(px(40.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .child(icon),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .min_w_0()
                                .child(
                                    ui::text(14., FontWeight::MEDIUM, ink::FULL)
                                        .truncate()
                                        .child(p.title.clone()),
                                )
                                .child(ui::text(13., FontWeight::NORMAL, 0.5).truncate().child(
                                    match p.kind {
                                        PackKind::Mod | PackKind::Shader => p.author.clone(),
                                        _ => tr(kind_name(p.kind)).to_string(),
                                    },
                                )),
                        )
                        .child(
                            ui::text(13., FontWeight::NORMAL, 0.6)
                                .truncate()
                                .child(p.summary.clone()),
                        ),
                )
                .child(right.flex_none())
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .justify_center()
                        .w(px(20.))
                        .when(installed, |d| d.child(ui::icon("tick-02", 17., ui::live())))
                        .when(busy && !installed, |d| d.child(ui::dot(ui::Tone::Busy))),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods.picked = Some(id.clone());
                    this.mods.confirm_remove = None;
                    cx.notify();
                })),
            );
        }
        if self.mods.more {
            let searching = self.mods.searching;
            let offset = self.mods.results.len();
            list = list.child(
                div().flex().p(px(8.)).child(
                    ui::quiet("mods-more", tr("More"), 36., &t)
                        .when(searching, |d| d.opacity(0.5))
                        .when(!searching, |d| {
                            d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.mods_search(offset);
                                cx.notify();
                            }))
                        }),
                ),
            );
        }
        list
    }

    /// The picked result, else the first; when a search finds nothing, the card shown last stays
    /// (the mockup's detail keeps its mod).
    fn mods_picked(&mut self) -> Option<Project> {
        let m = &mut self.mods;
        let found = m
            .picked
            .as_ref()
            .and_then(|id| m.results.iter().find(|p| &p.id == id))
            .or(m.results.first())
            .cloned();
        match found {
            Some(p) => {
                m.shown = Some(p.clone());
                Some(p)
            }
            None if m.searched && !m.searching && m.error.is_none() => m
                .shown
                .clone()
                .filter(|p| p.kind == m.kind && p.source == m.source),
            None => None,
        }
    }

    /// The picked project's card (`ModDetail`).
    fn glass_mod_detail(&mut self, instance: &str, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        let p = self.mods_picked()?;
        let icon = self.mods_icon(&p, 64.);
        let m = &self.mods;
        let support = m.support(&p);
        let entry = m.entry(&p).cloned();
        let busy = m.busy.as_ref().map(|b| b.0.clone());
        let checking = m.checking(&p);
        let mut by = Vec::new();
        if !p.author.is_empty() {
            by.push(trf("by {author}", &[("author", &p.author)]));
        }
        by.push(trf("from {source}", &[("source", p.source.label())]));
        if !p.license.is_empty() {
            by.push(p.license.clone());
        }
        let (tone, reason) = match &support {
            Support::Works => (ui::live(), None),
            Support::Partial(_) => (ui::warn(), support.reason()),
            Support::No(_) => (white(0.55), support.reason()),
        };
        let about = m
            .about
            .get(&p.id)
            .filter(|_| p.source == SourceId::Horizon)
            .map(|lines| lines.join(" · "));

        // The action: Remove (then Remove for good), Install or Import, or why not.
        let button: AnyElement = if let Some(e) = &entry {
            let confirming = m.confirm_remove.as_deref() == Some(e.project.as_str());
            let id = e.project.clone();
            ui::quiet(
                SharedString::from(format!("mods-detail-remove-{}", p.id)),
                if confirming {
                    tr("Remove for good").to_string()
                } else {
                    trf("Remove from {instance}", &[("instance", instance)])
                },
                44.,
                &t,
            )
            .w_full()
            .when(confirming, |d| d.text_color(ui::danger()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.mods_remove_click(id.clone());
                cx.notify();
            }))
            .into_any_element()
        } else if busy.as_deref() == Some(p.id.as_str()) || checking {
            ui::quiet(
                "mods-detail-wait",
                if checking {
                    tr("Checking…")
                } else {
                    tr("Installing…")
                },
                44.,
                &t,
            )
            .w_full()
            .opacity(0.6)
            .into_any_element()
        } else if support.installable() {
            let label = if p.kind == PackKind::Modpack {
                tr("Import as an instance").to_string()
            } else {
                trf("Install in {instance}", &[("instance", instance)])
            };
            let known = p.clone();
            ui::solid(
                SharedString::from(format!("mods-detail-install-{}", p.id)),
                label,
                44.,
                14.,
                &t,
            )
            .w_full()
            .when(busy.is_some(), |d| d.opacity(0.5))
            .when(busy.is_none(), |d| {
                d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_install(known.source, known.id.clone(), Some(known.clone()));
                    cx.notify();
                }))
            })
            .into_any_element()
        } else {
            ui::quiet("mods-detail-cannot", tr(support.label()), 44., &t)
                .w_full()
                .opacity(0.5)
                .into_any_element()
        };
        let open = p.clone();
        Some(
            crate::glass::plate(radius::PANEL)
                .flex()
                .flex_col()
                .p(px(24.))
                .child(
                    div()
                        .size(px(64.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .child(icon),
                )
                .child(
                    div()
                        .mt(px(20.))
                        .font_family(horizon_theme::glass::DISPLAY_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(24.))
                        .line_height(px(30.))
                        .text_color(white(ink::FULL))
                        .child(p.title.clone()),
                )
                .child(
                    ui::text(13., FontWeight::NORMAL, 0.55)
                        .mt(px(4.))
                        .child(by.join(" · ")),
                )
                .children(
                    (p.source == SourceId::Horizon)
                        .then(|| stars(&p.id, &t))
                        .flatten()
                        .map(|s| s.mt(px(8.))),
                )
                .child(
                    ui::text(14., FontWeight::NORMAL, 0.75)
                        .mt(px(16.))
                        .line_height(px(22.4))
                        .child(p.summary.clone()),
                )
                .children(about.map(|a| {
                    ui::text(13., FontWeight::NORMAL, 0.55)
                        .mt(px(16.))
                        .line_height(px(20.))
                        .child(a)
                }))
                .child(
                    div()
                        .mt(px(16.))
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tone)
                        .child(trf(
                            "{support} in Horizon",
                            &[("support", tr(support.label()))],
                        )),
                )
                .children(reason.map(|r| {
                    ui::text(13., FontWeight::NORMAL, 0.55)
                        .mt(px(4.))
                        .child(r.to_string())
                }))
                .child(
                    ui::text(13., FontWeight::NORMAL, 0.55)
                        .mt(px(16.))
                        .child(trf(
                            "{count} downloads",
                            &[("count", &super::thousands(p.downloads))],
                        )),
                )
                .child(div().mt(px(24.)).child(button))
                .child(
                    div().flex().justify_center().mt(px(12.)).child(
                        ui::link(
                            SharedString::from(format!("mods-detail-page-{}", p.id)),
                            tr("Its page"),
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.mods_open(open.clone());
                                cx.notify();
                            },
                        )),
                    ),
                ),
        )
    }
}

/// The mockup's `Tabs` at size `sm`: 32 px, 13 px, white at 12 % on the open one.
fn small_tab(id: SharedString, open: bool, t: &theme::Theme) -> Stateful<Div> {
    keyboard(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(32.))
            .px(px(12.))
            .rounded(px(10.))
            .cursor_pointer()
            .font_family(horizon_theme::glass::FONT)
            .text_size(px(13.))
            .when(open, |d| {
                d.bg(white(0.12))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(white(ink::FULL))
            })
            .when(!open, |d| {
                d.text_color(white(0.7))
                    .hover(|s| s.text_color(white(ink::FULL)))
            }),
        t,
    )
}

/// One project's kind, for a Modrinth result's second word (`Resource pack`).
fn kind_name(kind: PackKind) -> &'static str {
    match kind {
        PackKind::Mod => "Mod",
        PackKind::Modpack => "Modpack",
        PackKind::ResourcePack => "Resource pack",
        PackKind::Shader => "Shader pack",
        PackKind::DataPack => "Data pack",
    }
}
