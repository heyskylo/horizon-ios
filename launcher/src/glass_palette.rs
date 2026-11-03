//! The palette (⌘K, Ctrl+K elsewhere): the mockup's `Palette`. A glass card 560 px wide at 14 %
//! of the window's height, over the dimmed launcher: a search line, then up to eight commands,
//! each its name and a hint. Arrows move, Enter runs, Esc or a click outside closes.
//!
//! The commands are the mockup's: Play each instance, Open each page and Settings, New
//! instance, Open the game folder, and from two letters on, Install or Remove the registry's
//! mods that match, in the selected instance.

use super::line_edit::{self, Key, LineEdit};
use super::*;
use crate::glass_ui::{self as ui, white};
use horizon_launcher::browser::Project;
use horizon_launcher::instances::Kind as InstanceKind;
use horizon_theme::glass::{ink, SCRIM};
use std::path::PathBuf;

/// Commands listed at most (`slice(0, 8)`).
const SHOWN: usize = 8;

pub(super) struct Palette {
    query: LineEdit,
    focus: FocusHandle,
    /// The highlighted command.
    index: usize,
    /// The registry's mods for the query [`asked`](Self::asked) last.
    pub(super) mods: Vec<Project>,
    asked: String,
    /// The registry search's number: an answer to an older one is dropped.
    pub(super) search: u64,
}

#[derive(Clone)]
enum Run {
    Play(String),
    Go(Page),
    NewInstance,
    Folder(PathBuf),
    Install(Box<Project>),
    Remove(String),
}

struct Command {
    label: String,
    hint: String,
    run: Run,
}

impl Launcher {
    /// ⌘K: the palette opens, or closes when it is open.
    pub(super) fn palette_toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            window.focus(&self.root_focus, cx);
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.palette = Some(Palette {
            query: LineEdit::new(String::new(), 80, 40),
            focus,
            index: 0,
            mods: Vec::new(),
            asked: String::new(),
            search: 0,
        });
    }

    fn palette_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        window.focus(&self.root_focus, cx);
    }

    /// The commands for `q` (lower case, trimmed), filtered as the mockup does.
    fn palette_commands(&self, q: &str) -> Vec<Command> {
        let instance = self
            .selected_instance()
            .map_or_else(|| "Horizon".to_owned(), |i| i.name.clone());
        let mut all = Vec::new();
        for i in &self.instances.list {
            if matches!(i.kind, InstanceKind::Default | InstanceKind::Own) {
                all.push(Command {
                    label: trf("Play {name}", &[("name", &i.name)]),
                    hint: tr("Instance").to_string(),
                    run: Run::Play(i.id.clone()),
                });
            }
        }
        // `in_top_bar`: a page the top bar hides stays out of the palette too.
        for (page, label, _) in
            Page::in_top_bar(self.wardrobe_shown()).chain([(Page::Settings, "Settings", "")])
        {
            all.push(Command {
                label: trf("Open {page}", &[("page", tr(label))]),
                hint: tr("Page").to_string(),
                run: Run::Go(page),
            });
        }
        all.push(Command {
            label: tr("New instance").to_string(),
            hint: tr("Instance").to_string(),
            run: Run::NewInstance,
        });
        if let Some(home) = self.selected_instance().map(|i| i.home.clone()) {
            all.push(Command {
                label: tr("Open the game folder").to_string(),
                hint: crate::glass_toast::tilde(&home),
                run: Run::Folder(home),
            });
        }
        if q.chars().count() >= 2 {
            let mods = self.palette.as_ref().map_or(&[][..], |p| &p.mods[..]);
            for p in mods.iter().filter(|p| p.title.to_lowercase().contains(q)) {
                let has = self.mods_has(p);
                all.push(Command {
                    label: if has {
                        trf("Remove {name}", &[("name", &p.title)])
                    } else {
                        trf("Install {name}", &[("name", &p.title)])
                    },
                    hint: trf("Mod · {instance}", &[("instance", &instance)]),
                    run: if has {
                        Run::Remove(p.id.clone())
                    } else {
                        Run::Install(Box::new(p.clone()))
                    },
                });
            }
        }
        all.retain(|c| {
            q.is_empty() || c.label.to_lowercase().contains(q) || c.hint.to_lowercase().contains(q)
        });
        all.truncate(SHOWN);
        all
    }

    fn palette_run(&mut self, run: Run, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_close(window, cx);
        match run {
            Run::Play(id) => {
                let Some(instance) = self.instances.list.iter().find(|i| i.id == id).cloned()
                else {
                    return;
                };
                self.select_instance(&instance);
                self.go(Page::Play);
                // Play only when the Play button would; otherwise the page says what it needs.
                let running = self.client.is_some() || self.starting_client.is_some();
                let prepared = self.selected().is_some_and(|t| backend::is_prepared(t));
                if instance.java.is_some() || starts_now(prepared, self.preparing, running) {
                    self.play();
                }
            }
            Run::Go(page) => self.go(page),
            Run::NewInstance => self.open_instance_dialog(None),
            Run::Folder(home) => {
                let _ = std::fs::create_dir_all(&home);
                crate::glass_toast::open(cx, &home);
            }
            Run::Install(p) => {
                let p = *p;
                self.mods_install(p.source, p.id.clone(), Some(p));
            }
            Run::Remove(id) => {
                let _ = self.mods_remove(&id);
            }
        }
    }

    /// From two letters on, the registry is asked once for each new query.
    fn palette_ask(&mut self) {
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let q = p.query.text().trim().to_lowercase();
        if q.chars().count() < 2 || q == p.asked {
            return;
        }
        p.asked = q.clone();
        p.search += 1;
        let search = p.search;
        self.palette_mods(&q, search);
    }

    pub(super) fn palette_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let p = self.palette.as_ref()?;
        let t = self.theme;
        let raw = p.query.text().to_string();
        let q = raw.trim().to_lowercase();
        let focused = p.focus.is_focused(window);
        let focus = p.focus.clone();
        let input = line_edit::content(
            &p.query,
            &t,
            focused,
            false,
            false,
            "Play, install a mod, open a page",
        );
        let list = self.palette_commands(&q);
        let index = self
            .palette
            .as_ref()
            .map_or(0, |p| p.index)
            .min(list.len().saturating_sub(1));
        let runs: Vec<Run> = list.iter().map(|c| c.run.clone()).collect();
        let empty = list.is_empty();
        let size = window.viewport_size();
        let (w, h) = (f32::from(size.width), f32::from(size.height));

        let field = div()
            .id("palette-search")
            .track_focus(&focus)
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(48.))
            .px(px(12.))
            .cursor_text()
            .font_family(horizon_theme::glass::FONT)
            .text_size(px(15.))
            .text_color(white(ink::FULL))
            .child(ui::icon("search-01", 20., white(0.55)))
            .child(div().flex_1().min_w_0().overflow_hidden().child(input))
            .child(ui::text(12., FontWeight::NORMAL, 0.4).child(tr("Esc")))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                let Some(p) = this.palette.as_mut() else {
                    return;
                };
                match ev.keystroke.key.as_str() {
                    "down" => p.index = p.index.saturating_add(1),
                    "up" => p.index = p.index.saturating_sub(1),
                    _ => match line_edit::handle(&mut p.query, ev, cx) {
                        Key::Enter => {
                            let q = p.query.text().trim().to_lowercase();
                            let index = p.index;
                            let list = this.palette_commands(&q);
                            if let Some(c) = list.into_iter().nth(index) {
                                this.palette_run(c.run, window, cx);
                            }
                        }
                        Key::Escape => this.palette_close(window, cx),
                        Key::Edited => {
                            p.index = 0;
                            this.palette_ask();
                        }
                        Key::Pass => return,
                        _ => {}
                    },
                }
                // Past the last command the highlight stays on it.
                let q = this
                    .palette
                    .as_ref()
                    .map(|p| p.query.text().trim().to_lowercase());
                if let Some(q) = q {
                    let last = this.palette_commands(&q).len().saturating_sub(1);
                    if let Some(p) = this.palette.as_mut() {
                        p.index = p.index.min(last);
                    }
                }
                cx.stop_propagation();
                cx.notify();
            }));

        let mut rows = div()
            .id("palette-list")
            .flex()
            .flex_col()
            .pt(px(4.))
            .max_h(px(h * 0.5))
            .overflow_y_scroll();
        for (i, (c, run)) in list.into_iter().zip(runs).enumerate() {
            rows = rows.child(
                div()
                    .id(("palette-row", i))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .h(px(44.))
                    .px(px(12.))
                    .rounded(px(12.))
                    .when(i == index, |d| d.bg(white(0.1)))
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if let (true, Some(p)) = (*hovered, this.palette.as_mut()) {
                            if p.index != i {
                                p.index = i;
                                cx.notify();
                            }
                        }
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.palette_run(run.clone(), window, cx);
                        cx.notify();
                    }))
                    .child(
                        ui::text(14., FontWeight::NORMAL, ink::FULL)
                            .min_w_0()
                            .truncate()
                            .child(c.label),
                    )
                    .child(
                        ui::text(13., FontWeight::NORMAL, 0.5)
                            .flex_none()
                            .child(c.hint),
                    ),
            );
        }
        if empty {
            rows = rows.child(
                ui::text(14., FontWeight::NORMAL, 0.6)
                    .px(px(12.))
                    .py(px(16.))
                    .child(trf("Nothing matches “{query}”.", &[("query", raw.trim())])),
            );
        }

        // The card is glass over the dimmed launcher (`hzl-glass` over the scrim): its piece of
        // the scene under the same dimming, and the plates' 6 % white.
        let card = crate::glass::plate(20.)
            .id("palette")
            .occlude()
            .w(px(560_f32.min(w - 32.)))
            .p(px(8.))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(20.))
                    .bg(ui::color(SCRIM).opacity(0.5)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(20.))
                    .bg(white(horizon_theme::glass::TINT)),
            )
            .child(div().relative().child(field).child(rows))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation());
        Some(
            div()
                .id("palette-scrim")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(h * 0.14))
                .occlude()
                .on_scroll(|_, _, cx| cx.stop_propagation())
                // `backdrop-blur-xl` over the launcher: GPUI can't blur what it drew, so the
                // scene's blurred copy covers it (as a menu's, `hzl-float`), then the scrim.
                .child(crate::glass::backdrop(crate::glass::Kind::Float, 0.))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(ui::color(SCRIM).opacity(0.5))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                this.palette_close(window, cx);
                                cx.notify();
                            }),
                        ),
                )
                .child(card),
        )
    }
}
