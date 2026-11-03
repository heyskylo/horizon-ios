//! The Servers page in the glass look (the mockup's `ServersView`, horizon-site
//! `components/launcher/launcher-views.tsx`): the open server's glass with its state, name, numbers,
//! Start or Restart and Stop, its console and command line; beside it the list of servers and how
//! friends reach the open one. Below, what the mockup has no room for: the server's Players,
//! Settings and Backups, then Sculk's banner (sponsor). Dark appearance only.
use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use horizon_theme::glass::{fill, ink, radius};

/// The right column's width (`lg:grid-cols-[minmax(0,1fr)_320px]`).
const SIDE: f32 = 320.;

/// The tabs under the server's glass: the mockup shows the Overview's parts on the page itself.
const LOWER_TABS: [(Tab, &str); 3] = [
    (Tab::Players, "Players"),
    (Tab::Settings, "Settings"),
    (Tab::Backups, "Backups"),
];

/// A dot's tone for a server's state.
fn tone(status: &Status) -> Tone {
    match status.phase.as_str() {
        "running" if status.crashes > 0 => Tone::Warn,
        "running" => Tone::Live,
        "stopped" => Tone::Off,
        _ => Tone::Busy,
    }
}

impl Launcher {
    pub(super) fn glass_servers_page(&mut self, cx: &mut Context<Self>) -> Div {
        let head = glass_ui::page_head(
            tr("Servers"),
            tr("Host a world for your friends with buttons and no commands. The same program runs on a VPS."),
        );
        // The mockup always shows a server: the open one, else the first.
        if self.servers.open.is_none() {
            if let Some(first) = self.servers.list.first() {
                let dir = first.dir.clone();
                self.open_server(dir);
            }
        }
        let open = self
            .servers
            .open
            .clone()
            .filter(|d| self.servers.list.iter().any(|e| &e.dir == d));
        let main = match &open {
            Some(dir) => self.glass_server(dir, cx),
            None => self.glass_no_server(cx),
        };
        let status = self.servers.status();
        let side = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDE))
            .gap(px(10.))
            .child(self.glass_server_list(cx))
            .children(open.is_some().then(|| self.glass_addresses(&status, cx)));
        let mut page = div().flex().flex_col().child(head).child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(main.flex_1().min_w_0())
                .child(side),
        );
        if let Some(dir) = &open {
            page = page.child(self.glass_server_tabs(dir, &status, cx));
        }
        page.children(self.glass_sculk(cx))
    }

    /// The open server's glass: its state, name and numbers, its buttons, then its console.
    fn glass_server(&mut self, dir: &Path, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let status = self.servers.status();
        let running = status.running();
        let idle = self.servers.busy.is_none();
        let name = self
            .servers
            .list
            .iter()
            .find(|e| e.dir == dir)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let mut state = phase_label(&status);
        if let Some(up) = status.uptime.filter(|_| running) {
            state = format!("{state} · {}", trf("up {time}", &[("time", &uptime(up))]));
        }
        let numbers = if running {
            let mut parts = vec![trf(
                "{online} of {max} players",
                &[
                    ("online", &status.players.len().to_string()),
                    ("max", &status.max_players.unwrap_or(0).to_string()),
                ],
            )];
            if let Some(tps) = status.tps {
                parts.push(trf("{tps} TPS", &[("tps", &format!("{tps:.1}"))]));
            }
            if let Some(mspt) = status.mspt {
                parts.push(trf("{ms} ms a tick", &[("ms", &format!("{mspt:.1}"))]));
            }
            if let Some(ram) = status.ram_bytes {
                parts.push(format!("{:.2} GB", ram as f64 / 1e9));
            }
            parts.join(" · ")
        } else {
            let mode = self.servers.shown("world", "game_mode");
            let mode = MODES
                .iter()
                .find(|(id, _)| *id == mode)
                .map_or(mode.clone(), |(_, label)| tr(label).to_string());
            let max = self.servers.shown("server", "max_players");
            [
                mode,
                self.servers.shown("world", "name"),
                if max.is_empty() {
                    max
                } else {
                    trf("up to {max} players", &[("max", &max)])
                },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
        };

        // The web panel and the folder (not in the mockup): small icon buttons in the glass's top
        // right corner, which the mockup leaves empty, so the head keeps its layout.
        let mut corner = div()
            .absolute()
            .top(px(16.))
            .right(px(16.))
            .flex()
            .gap(px(4.));
        let panel = ui::icon_button(
            "server-panel",
            "dashboard-square-01",
            32.,
            17.,
            10.,
            false,
            &t,
        );
        corner = corner.child(if running {
            let d = dir.to_path_buf();
            panel.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                match servers::Api::open(&d).map(|a| a.panel_link()) {
                    Some(Ok(url)) => cx.open_url(&url),
                    Some(Err(e)) => this.servers.problem = Some(Problem::plain(e)),
                    None => {}
                }
                cx.notify();
            }))
        } else {
            panel.opacity(0.4).cursor_default()
        });
        let folder = dir.to_path_buf();
        corner = corner.child(
            ui::icon_button("server-folder", "folder-01", 32., 17., 10., false, &t)
                .on_click(move |_, _, cx| crate::glass_toast::open(cx, &folder)),
        );
        // Start, or Restart and Stop.
        let mut buttons = div().flex().flex_none().items_center().gap(px(8.));
        let wait = |b: Stateful<Div>| b.opacity(0.4).cursor_default();
        if running {
            let restart = ui::quiet("server-restart", tr("Restart"), 44., &t).px(px(20.));
            buttons = buttons.child(if idle {
                restart.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.servers.restart_needed.clear();
                    this.server_command("Restarting", vec!["restart".into()]);
                    cx.notify();
                }))
            } else {
                wait(restart)
            });
            let stop = ui::solid("server-start", tr("Stop"), 44., 14., &t).px(px(24.));
            buttons = buttons.child(if idle {
                stop.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.server_command("Stopping", vec!["stop".into()]);
                    cx.notify();
                }))
            } else {
                wait(stop)
            });
        } else {
            let look = self.look;
            let start = keyboard(
                div()
                    .id("server-start")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .h(px(44.))
                    .px(px(28.))
                    .rounded(px(radius::LARGE))
                    .bg(ui::color(look.accent))
                    .text_color(ui::color(look.on_accent))
                    .font_family(horizon_theme::glass::FONT)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.92))
                    .child(tr("Start")),
                &t,
            );
            buttons = buttons.child(if idle && status.phase == "stopped" {
                start.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.start_server();
                    cx.notify();
                }))
            } else {
                start.opacity(0.5).cursor_default()
            });
        }

        let mut glass = glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .p(px(24.))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::dot(tone(&status)))
                    .child(
                        ui::text(13., FontWeight::NORMAL, ink::SOFT)
                            .line_height(px(19.5))
                            .child(state),
                    ),
            )
            .child(
                div()
                    .relative()
                    .mt(px(12.))
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(43.5))
                                    .line_height(px(43.5))
                                    .text_color(white(ink::FULL))
                                    .child(name),
                            )
                            .child(
                                ui::text(14., FontWeight::NORMAL, ink::BODY)
                                    .mt(px(12.))
                                    .line_height(px(21.))
                                    .truncate()
                                    .child(numbers),
                            ),
                    )
                    .child(buttons),
            )
            .child(corner);
        // What the launcher says besides (not in the mockup): a crash count, the action under way,
        // a problem, a note, settings that wait for a restart.
        if status.crashes > 0 {
            glass = glass.child(
                ui::card_note(trf(
                    "The game crashed {count} times and was restarted by itself.",
                    &[("count", &status.crashes.to_string())],
                ))
                .relative()
                .mt(px(8.)),
            );
        }
        glass = glass.children(self.glass_server_notes(running, idle, cx));
        glass.child(self.glass_server_console(running, cx))
    }

    /// The action under way, a problem, a note and settings waiting for a restart, each a line on
    /// white at 6 % (not in the mockup).
    fn glass_server_notes(
        &mut self,
        running: bool,
        idle: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let t = self.theme;
        let line = |dot: Option<Hsla>, text: String| {
            div()
                .relative()
                .mt(px(16.))
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(44.))
                .px(px(16.))
                .py(px(6.))
                .rounded(px(radius::ROW))
                .bg(white(fill::ROW_HOVER))
                .children(dot.map(|c| div().flex_none().size(px(7.)).rounded_full().bg(c)))
                .child(
                    ui::text(14., FontWeight::NORMAL, ink::FULL)
                        .flex_1()
                        .min_w_0()
                        .child(text),
                )
        };
        let mut out = Vec::new();
        if let Some(label) = self.servers.busy.clone() {
            let last = self.servers.job_log.back().cloned().unwrap_or_default();
            out.push(
                div()
                    .relative()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(progress_bar(None, &t))
                    .child(
                        ui::card_note(format!("{} {last}", tr(&format!("{label}…")))).truncate(),
                    ),
            );
        }
        if let Some(p) = self.servers.problem.clone() {
            let mut d = line(
                Some(ui::color(horizon_theme::glass::DANGER)),
                p.message.clone(),
            );
            if let Some(fix) = p.fix {
                let label = match &fix {
                    Fix::UsePort(port) => trf("Use port {port}", &[("port", &port.to_string())]),
                    Fix::Prepare => tr("Prepare the game files").into(),
                    Fix::AgreeEula => tr("I agree to the EULA").into(),
                    Fix::ShowLog => tr("Open the logs").into(),
                };
                d = d.child(
                    ui::quiet("problem-fix", label, 32., &t).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.fix(fix.clone(), cx);
                            cx.notify();
                        },
                    )),
                );
            }
            out.push(
                d.child(
                    button("problem-dismiss", "Dismiss", Kind::Ghost, &t)
                        .h(px(32.))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.servers.problem = None;
                            cx.notify();
                        })),
                ),
            );
        }
        if let Some(n) = self.servers.note.clone() {
            out.push(line(Some(ui::color(horizon_theme::glass::LIVE)), n));
        }
        if running && !self.servers.restart_needed.is_empty() {
            let names: Vec<String> = self
                .servers
                .restart_needed
                .iter()
                .map(|k| tr(setting_name(k)).to_string())
                .collect();
            let restart = ui::quiet("restart-apply", tr("Restart now"), 32., &t);
            out.push(
                line(
                    Some(ui::color(self.look.accent_soft)),
                    trf(
                        "Saved. Restart to apply: {settings}.",
                        &[("settings", &names.join(", "))],
                    ),
                )
                .child(if idle {
                    restart.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.servers.restart_needed.clear();
                        this.server_command("Restarting", vec!["restart".into()]);
                        cx.notify();
                    }))
                } else {
                    restart.opacity(0.4).cursor_default()
                }),
            );
        }
        out
    }

    /// The console (`Logs`, 320 px) or its empty box, and the command line under it.
    fn glass_server_console(&mut self, running: bool, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let body = if self.servers.log.is_empty() {
            div()
                .flex()
                .flex_col()
                .items_start()
                .justify_center()
                .gap(px(8.))
                .h(px(320.))
                .p(px(24.))
                .rounded(px(radius::ROW))
                .bg(gpui::black().opacity(0.25))
                .child(ui::icon("computer-terminal-01", 22., white(ink::NOTE)))
                .child(
                    ui::text(14., FontWeight::NORMAL, ink::NOTE).child(if running {
                        tr("Waiting for the server's log…")
                    } else {
                        tr("The console shows up here once the server starts.")
                    }),
                )
                .into_any_element()
        } else {
            let lines: Vec<Div> = self
                .servers
                .log
                .iter()
                .map(|l| {
                    div()
                        .when(l.starts_with("> "), |d| {
                            d.text_color(ui::color(self.look.accent_soft))
                        })
                        .child(l.clone())
                })
                .collect();
            let log = div()
                .id("server-console")
                .h(px(320.))
                .p(px(16.))
                .overflow_y_scroll()
                .rounded(px(radius::ROW))
                .bg(gpui::black().opacity(0.25))
                .font_family(theme::MONO)
                .text_size(px(12.))
                .line_height(px(21.))
                .text_color(white(ink::SOFT))
                .children(lines);
            self.servers.console_scroll.wrap(log, &t).into_any_element()
        };
        let send = ui::quiet("server-send", tr("Send"), 32., &t)
            .px(px(12.))
            .text_size(px(13.));
        let typed = !self.servers.value(Field::Command).trim().is_empty();
        let send = if running && typed {
            send.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.field_enter(Field::Command);
                cx.notify();
            }))
        } else {
            send.opacity(0.4).cursor_default()
        };
        let input = if running {
            self.field(
                Field::Command,
                "Type a command, like list or time set day",
                520.,
                cx,
            )
            .flex_1()
            .min_w_0()
            .into_any_element()
        } else {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(white(ink::PLACEHOLDER))
                .child(tr("Start the server to send commands"))
                .into_any_element()
        };
        let focused = self.servers.focused == Some(Field::Command);
        div()
            .relative()
            .mt(px(24.))
            .flex()
            .flex_col()
            .child(body)
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(44.))
                    .pl(px(16.))
                    .pr(px(6.))
                    .rounded(px(radius::ROW))
                    .bg(white(if focused {
                        fill::FIELD_FOCUS
                    } else {
                        fill::FIELD
                    }))
                    .font_family(theme::MONO)
                    .text_size(px(13.))
                    .child(div().text_color(white(ink::PLACEHOLDER)).child("/"))
                    .child(input)
                    .child(send),
            )
    }

    /// No server yet: the glass says so, with Create (not in the mockup, which always has one).
    fn glass_no_server(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut d = glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .items_start()
            .gap(px(16.))
            .p(px(24.))
            .child(
                ui::text(14., FontWeight::NORMAL, ink::NOTE)
                    .relative()
                    .line_height(px(21.))
                    .child(tr("No server yet. Create one: it takes a few seconds, and with the defaults (tick the EULA, press Create) you get a friends-only survival world.")),
            );
        if let Some(p) = self.servers.problem.clone() {
            d = d.child(glass_home_lists::danger_line(p.message).relative());
        }
        d.child(
            button("server-create-empty", "Create a server", Kind::Primary, &t)
                .h(px(44.))
                .px(px(24.))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.open_create(cx);
                    cx.notify();
                })),
        )
    }

    /// The servers, the open one lit, then Create a server and (not in the mockup) adding a
    /// server folder.
    fn glass_server_list(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut list = glass::plate(radius::PANEL).flex().flex_col().p(px(8.));
        let row = |id: SharedString, current: bool| {
            let d = div()
                .id(id)
                .relative()
                .flex()
                .items_center()
                .gap(px(12.))
                .w_full()
                .px(px(12.))
                .py(px(10.))
                .rounded(px(radius::ROW))
                .cursor_pointer();
            if current {
                d.bg(white(fill::SELECTED))
            } else {
                d.hover(|s| s.bg(white(fill::ROW_HOVER)))
            }
        };
        for (i, e) in self.servers.list.clone().into_iter().enumerate() {
            let status = self
                .servers
                .statuses
                .get(&e.dir)
                .cloned()
                .unwrap_or_else(Status::stopped);
            let current = self.servers.open.as_ref() == Some(&e.dir);
            let line = match status.phase.as_str() {
                "running" => trf(
                    "{online} of {max} online",
                    &[
                        ("online", &status.players.len().to_string()),
                        ("max", &status.max_players.unwrap_or(0).to_string()),
                    ],
                ),
                "stopped" => self.servers.stopped_line(&e.dir),
                _ => phase_label(&status),
            };
            let dir = e.dir.clone();
            list = list.child(
                keyboard(
                    row(SharedString::from(format!("server-row-{i}")), current),
                    &t,
                )
                .child(ui::list_text(e.name.clone(), line))
                .child(ui::dot(tone(&status)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.servers.polled = None;
                    this.servers.problem = None;
                    this.servers.note = None;
                    this.open_server(dir.clone());
                    cx.notify();
                })),
            );
        }
        let action = |id: &'static str, icon: &'static str, label: SharedString| {
            keyboard(row(id.into(), false), &t)
                .child(ui::icon(icon, 16., white(ink::BODY)))
                .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(label))
        };
        list = list.child(
            action("server-create", "plus-sign", tr("Create a server").into()).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.open_create(cx);
                    cx.notify();
                }),
            ),
        );
        let add = action("server-add", "folder-01", tr("Add a server folder…").into());
        list.child(if self.servers.creating {
            add.opacity(0.4).cursor_default()
        } else {
            add.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.add_server_folder(cx);
                cx.notify();
            }))
        })
    }

    /// How friends reach the open server: on this network, over the internet (Tailscale), and who
    /// is online now.
    fn glass_addresses(&mut self, status: &Status, cx: &mut Context<Self>) -> Div {
        let port = self
            .servers
            .setting("server", "port")
            .and_then(Value::as_u64)
            .unwrap_or(25565) as u16;
        let lan = match servers::lan_address() {
            Some(ip) if port == 25565 => ip.to_string(),
            Some(ip) => format!("{ip}:{port}"),
            None => format!("localhost:{port}"),
        };
        let copied = self
            .servers
            .copied
            .is_some_and(|at| at.elapsed() < Duration::from_secs(2));
        let copy = |this: &Self, id: &'static str, text: String, cx: &mut Context<Self>| {
            ui::icon_button(
                id,
                if copied { "tick-02" } else { "copy-01" },
                36.,
                17.,
                10.,
                false,
                &this.theme,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                copy_to_clipboard(cx, gpui::ClipboardItem::new_string(text.clone()));
                this.servers.copied = Some(Instant::now());
                cx.notify();
            }))
        };
        let address = |value: String, size: f32, button: Stateful<Div>| {
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    ui::text(size, FontWeight::MEDIUM, ink::FULL)
                        .min_w_0()
                        .truncate()
                        .child(value),
                )
                .child(button)
        };
        let here = div()
            .flex()
            .flex_col()
            .child(ui::card_heading(tr("On this network")))
            .child(address(
                lan.clone(),
                22.,
                copy(self, "address-copy", lan, cx),
            ))
            .child(
                ui::card_note(tr(
                    "Friends on the same Wi-Fi: Multiplayer, Add server, paste.",
                ))
                .mt(px(4.)),
            );
        let internet = self.glass_internet(port, cx);
        let mut card = glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .gap(px(24.))
            .p(px(20.))
            .child(here.relative())
            .child(internet.relative());
        if status.running() && !status.players.is_empty() {
            let ops = self
                .servers
                .open
                .as_deref()
                .map(|d| servers::names(d, "ops.json"))
                .unwrap_or_default();
            let mut people = div().mt(px(12.)).flex().flex_col().gap(px(10.));
            for p in status.players.clone() {
                let op = ops.iter().any(|o| o.eq_ignore_ascii_case(&p.name));
                people = people.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(self.head(&p.uuid, 28.).rounded(px(7.)))
                        .child(
                            ui::text(14., FontWeight::NORMAL, ink::FULL)
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(p.name.clone()),
                        )
                        .child(ui::text(13., FontWeight::NORMAL, ink::FAINT).child(if op {
                            tr("Operator")
                        } else {
                            tr("Friend")
                        })),
                );
            }
            card = card.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(ui::card_heading(tr("Online now")))
                    .child(people),
            );
        }
        card
    }

    /// The server's Players, Settings and Backups (not in the mockup), on a glass under it.
    fn glass_server_tabs(&mut self, dir: &Path, status: &Status, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        if self.servers.tab == Tab::Overview {
            self.servers.tab = Tab::Players;
        }
        let strip = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(LOWER_TABS.iter().map(|&(tab, label)| {
                ui::tab(
                    SharedString::from(format!("server-tab-{label}")),
                    tr(label),
                    self.servers.tab == tab,
                    &t,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.servers.tab = tab;
                    this.servers.confirm_restore = None;
                    cx.notify();
                }))
            }));
        let players = !matches!(self.servers.tab, Tab::Settings | Tab::Backups);
        let body = match self.servers.tab {
            Tab::Settings => self.settings_tab(status, cx),
            Tab::Backups => self.backups_tab(status, cx),
            _ => self.players_tab(dir, status, cx),
        };
        glass::plate(radius::PANEL)
            .mt(px(10.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .p(px(24.))
            .child(strip.relative())
            .child(body.relative())
            .when(players, |d| {
                d.child(self.internet_invites(port_of(self), cx).relative())
            })
    }

    /// Sculk's banner (sponsor, not in the mockup) on a glass card: "Partner" as the cards' small
    /// headings, Hide once it has been on screen a while, the banner Sculk made.
    fn glass_sculk(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if self.settings.partner_hidden {
            return None;
        }
        let t = self.theme;
        let width = 600.;
        let mut head = div()
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .child(ui::card_heading(tr("Partner")));
        if self.settings.partner_seen_secs >= partner::SEEN_BEFORE_HIDE {
            head = head.child(
                button("partner-servers-hide", "Hide", Kind::Ghost, &t)
                    .h(px(28.))
                    .px(px(10.))
                    .text_size(px(13.))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings.partner_hidden = true;
                        this.save_settings();
                        cx.notify();
                    })),
            );
        }
        Some(
            glass::plate(radius::PANEL)
                .mt(px(10.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .w(px(width + 40.))
                .p(px(20.))
                .child(head)
                .child(partner::banner_image("partner-servers", width, &t).relative()),
        )
    }
}

/// The open server's port.
fn port_of(this: &Launcher) -> u16 {
    this.servers
        .setting("server", "port")
        .and_then(Value::as_u64)
        .unwrap_or(25565) as u16
}

/// A settings row in the glass look, the mockup's `SettingRow`: the name in 15 px semibold, one
/// line on it in 13 px at 60 % (or the refusal in red), the control on the right, 40 px apart.
pub(super) fn glass_setting_row(
    name: &str,
    about: String,
    error: Option<&'static str>,
    control: AnyElement,
) -> Div {
    let note = match error {
        Some(e) => ui::card_note(tr(e)).text_color(gpui::rgb(horizon_theme::glass::DANGER)),
        None => ui::card_note(about).text_color(white(0.6)),
    };
    div()
        .flex()
        .items_center()
        .gap(px(40.))
        .py(px(16.))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .child(
                    ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                        .line_height(px(21.))
                        .child(tr(name).to_string()),
                )
                .child(note),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
}

impl Launcher {
    /// A tab's section heading: the mockup's small card heading in the glass look.
    pub(super) fn tab_heading(&self, label: impl Into<SharedString>) -> Div {
        if self.theme.dark {
            return ui::card_heading(label);
        }
        text(theme::HEADING)
            .text_color(self.theme.ink)
            .child(label.into())
    }

    /// A switch with its name and note: a `SettingRow` in the glass look, the switch first in
    /// the other.
    pub(super) fn toggle_row(&self, name: &str, about: String, toggle: Stateful<Div>) -> Div {
        let t = self.theme;
        if t.dark {
            return glass_setting_row(name, about, None, toggle.into_any_element());
        }
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(toggle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        text(theme::UI)
                            .text_color(t.ink)
                            .child(tr(name).to_string()),
                    )
                    .child(text(theme::CAPTION).text_color(t.muted).child(about)),
            )
    }

    /// A row of a tab's list (backups, banned players): the mockup's `ROW` text in the glass
    /// look (14 px medium, its detail in 13 px at 55 %), with no plate or rule.
    pub(super) fn list_row(
        &self,
        lead: Option<Div>,
        name: impl IntoElement,
        detail: Option<AnyElement>,
        right: Option<AnyElement>,
    ) -> Div {
        let t = self.theme;
        if !t.dark {
            return slot_row(lead, name, detail, right, &t);
        }
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .py(px(10.))
            .children(lead)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(name))
                    .children(
                        detail.map(|d| ui::text(13., FontWeight::NORMAL, ink::QUIET).child(d)),
                    ),
            )
            .children(right)
    }

    /// − value + in the glass look: the Create dialog's stepper (a strip with 40 px buttons).
    pub(super) fn glass_stepper(
        &self,
        key: &'static str,
        value: i64,
        range: std::ops::RangeInclusive<i64>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        let step = |id: &'static str, icon: &'static str, to: i64, cx: &mut Context<Self>| {
            let b = keyboard(
                div()
                    .id(SharedString::from(format!("{key}-{id}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::BUTTON)))
                    .child(ui::icon(icon, 16., white(ink::FULL))),
                &t,
            );
            if range.contains(&to) && to != value {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_setting(key, to.to_string());
                    cx.notify();
                }))
            } else {
                b.opacity(0.35).cursor_default()
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .p(px(4.))
            .rounded(px(radius::LARGE))
            .bg(white(0.04))
            .child(step("less", "minus-sign", value - 1, cx))
            .child(
                ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                    .w(px(44.))
                    .flex()
                    .justify_center()
                    .child(value.to_string()),
            )
            .child(step("more", "plus-sign", value + 1, cx))
            .into_any_element()
    }
}
