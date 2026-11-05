//! The Create a server dialog in the glass look (the mockup's `ServerModal`, horizon-site
//! `components/launcher/server-modal.tsx`): one plate round at 28 over the dimmed page. On the
//! left, what the server will be (a picture, its name and its rules); on the right, its name,
//! world, game mode, difficulty, how many players, who may join and whether accounts are
//! checked; the EULA, Cancel and Create at the foot. Dark appearance only.
use super::*;
use crate::glass_instance::{choice, choice_text, list_box, list_choice, section, tick};
use crate::glass_ui::{self as ui, white};
use gpui::{img, ImageFormat};
use horizon_theme::glass::{fill, ink, radius};
use std::sync::{Arc, LazyLock};

/// The mockup's pictures for each game mode (`COVERS`): Horizon screenshots from `docs/media`.
static COVERS: LazyLock<[Arc<gpui::Image>; 3]> = LazyLock::new(|| {
    let webp = |bytes: &[u8]| Arc::new(gpui::Image::from_bytes(ImageFormat::Webp, bytes.to_vec()));
    [
        webp(include_bytes!("../../docs/media/village.webp")),
        webp(include_bytes!("../../docs/media/underwater.webp")),
        webp(include_bytes!("../../docs/media/nether.webp")),
    ]
});

/// Each game mode's line under its name (`MODES`).
const MODE_NOTES: [&str; 3] = [
    "Gather, build and stay alive",
    "Fly and build with every block",
    "Play a map the way it was made",
];

/// A player name Minecraft accepts: 3 to 16 letters, digits or underscores.
fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A click that changes the draft with `f`.
fn set(
    cx: &mut Context<Launcher>,
    f: fn(&mut Create),
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let this = cx.entity().downgrade();
    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
        let _ = this.update(cx, |this, cx| {
            if let Some(c) = &mut this.servers.create {
                f(c);
            }
            cx.notify();
        });
    }
}

impl Launcher {
    pub(super) fn glass_create_dialog(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let creating = self.servers.creating;
        self.servers.create.as_ref()?;
        let aside = self.create_aside();
        let main = self.create_main(cx);
        let close = cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            if !creating {
                this.servers.create = None;
                this.servers.problem = None;
            }
            cx.notify();
        });
        let card = ui::dialog_plate(radius::MODAL)
            .id("create-card")
            .occlude()
            .flex()
            .w_full()
            .max_w(px(1180.))
            .h_full()
            .max_h(px(840.))
            .child(aside)
            .child(main);
        Some(
            div()
                .id("create-dialog")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .occlude()
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .child(ui::scrim_backdrop())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(ui::scrim())
                        .on_mouse_down(gpui::MouseButton::Left, close),
                )
                .child(card),
        )
    }

    /// The left column: the picture, the name, the rules and where the world comes from.
    fn create_aside(&mut self) -> Stateful<Div> {
        let Some(c) = self.servers.create.as_ref() else {
            return div().id("create-aside");
        };
        let typed = self.servers.value(Field::Name).trim().to_string();
        let title = if typed.is_empty() {
            tr("My server").to_string()
        } else {
            typed
        };
        let label = |list: &[(&str, &'static str)], id: &str| {
            list.iter()
                .find(|(v, _)| *v == id)
                .map_or(String::new(), |(_, l)| tr(l).to_string())
        };
        let rules = [
            label(&MODES, c.mode),
            label(&DIFFICULTIES, c.difficulty),
            trf(
                "up to {max} players",
                &[("max", &c.max_players.to_string())],
            ),
            tr(if c.friends_only {
                "friends only"
            } else {
                "anyone with the address"
            })
            .to_string(),
        ]
        .join(" · ");
        let copy = c
            .copy
            .as_ref()
            .filter(|_| !c.new_world)
            .and_then(|p| c.worlds.iter().find(|(_, path)| path == p));
        let seed = self.servers.value(Field::Seed).trim().to_string();
        let origin = match (copy, seed.is_empty()) {
            (Some((folder, path)), _) => {
                let name = c.details.get(path).map_or(folder, |d| &d.0);
                trf("A copy of {name}", &[("name", name)])
            }
            (None, false) => trf("New world, seed {seed}", &[("seed", &seed)]),
            (None, true) => tr("A new world with a random seed").to_string(),
        };
        // A copied world shows its own icon, else the mode's picture.
        let cover = match copy
            .map(|(_, p)| p.join("icon.png"))
            .filter(|p| p.is_file())
        {
            Some(icon) => img(icon)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
            None => {
                let i = MODES.iter().position(|(v, _)| *v == c.mode).unwrap_or(0);
                img(COVERS[i].clone())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element()
            }
        };
        div()
            .id("create-aside")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(360.))
            .h_full()
            .p(px(28.))
            .overflow_y_scroll()
            .rounded_l(px(radius::MODAL))
            .bg(white(0.035))
            .child(ui::text(13., FontWeight::MEDIUM, ink::QUIET).child(tr("New server")))
            .child(
                div()
                    .mt(px(24.))
                    .w(px(304.))
                    .h(px(190.))
                    .flex_none()
                    .overflow_hidden()
                    .rounded(px(18.))
                    .child(div().size_full().child(cover)),
            )
            .child(
                div()
                    .mt(px(20.))
                    .truncate()
                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(30.))
                    .line_height(px(32.))
                    .text_color(white(ink::FULL))
                    .child(title),
            )
            .child(
                ui::text(14., FontWeight::NORMAL, ink::MUTED)
                    .mt(px(8.))
                    .line_height(px(21.))
                    .child(rules),
            )
            .child(
                ui::text(14., FontWeight::NORMAL, ink::FAINT)
                    .mt(px(4.))
                    .truncate()
                    .child(origin),
            )
            .child(div().flex_1().min_h(px(32.)))
            .child(
                ui::text(13., FontWeight::NORMAL, ink::FAINT)
                    .line_height(px(20.))
                    .child(tr("Everything has a default: tick the EULA and Create makes a friends-only survival world. You can change all of it later, from the server's Settings tab.")),
            )
    }

    /// The right column: the title, every choice, and the foot.
    fn create_main(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let Some(c) = self.servers.create.as_ref() else {
            return div();
        };
        let (new_world, copy, worlds) = (c.new_world, c.copy.clone(), c.worlds.clone());
        let details = c.details.clone();
        let (mode, difficulty, max) = (c.mode, c.difficulty, c.max_players);
        let (friends_only, check, friends) = (c.friends_only, c.check_accounts, c.friends.clone());
        let creating = self.servers.creating;
        // The mockup's `clamp(1.6rem, 2.4vw, 2.1rem)`.
        let size = (self.window_width * 0.024).clamp(25.6, 33.6);
        let head = div()
            .flex()
            .flex_none()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .px(px(36.))
            .pt(px(32.))
            .pb(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .font_family(horizon_theme::glass::DISPLAY_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(size))
                            .line_height(px(size * 1.25))
                            .text_color(white(ink::FULL))
                            .child(tr("Create a server")),
                    )
                    .child(
                        ui::text(14., FontWeight::NORMAL, ink::NOTE)
                            .mt(px(6.))
                            .child(tr(
                                "The same program a VPS runs, started from here with buttons.",
                            )),
                    ),
            )
            .child(
                ui::icon_button(
                    "create-close",
                    "cancel-01",
                    36.,
                    18.,
                    radius::CONTROL,
                    false,
                    &t,
                )
                .when(!creating, |b| {
                    b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.servers.create = None;
                        this.servers.problem = None;
                        cx.notify();
                    }))
                }),
            );

        // The world: new (with a seed) or a copy of one of the player's.
        let card = |id: &'static str, on: bool, title: SharedString, note: String| {
            choice(id, on, &t)
                .flex_1()
                .p(px(16.))
                .child(choice_text(title, note))
        };
        let count = if worlds.is_empty() {
            tr("No worlds yet").to_string()
        } else {
            trf(
                "{count} worlds to pick from",
                &[("count", &worlds.len().to_string())],
            )
        };
        let mut copy_card = card(
            "create-world-copy",
            !new_world,
            tr("Copy one of my worlds").into(),
            count,
        );
        copy_card = if worlds.is_empty() {
            copy_card.opacity(0.4).cursor_default()
        } else {
            copy_card.on_click(set(cx, |c| c.new_world = false))
        };
        let kinds = div()
            .flex()
            .gap(px(10.))
            .child(
                card(
                    "create-world-new",
                    new_world,
                    tr("New world").into(),
                    tr("Generated the first time it starts").to_string(),
                )
                .on_click(set(cx, |c| c.new_world = true)),
            )
            .child(copy_card);
        let detail = if new_world {
            self.field(
                Field::Seed,
                "Seed, optional: leave empty for a random world",
                400.,
                cx,
            )
            .w_full()
            .into_any_element()
        } else {
            let mut list = list_box("create-worlds", 232.);
            for (i, (name, path)) in worlds.into_iter().enumerate() {
                let on = copy.as_ref() == Some(&path);
                let icon = path.join("icon.png");
                let thumb = div()
                    .flex_none()
                    .w(px(64.))
                    .h(px(40.))
                    .overflow_hidden()
                    .rounded(px(8.))
                    .bg(white(fill::BUTTON))
                    .when(icon.is_file(), |d| {
                        d.child(img(icon).size_full().object_fit(gpui::ObjectFit::Cover))
                    });
                list = list.child(
                    list_choice(SharedString::from(format!("create-copy-{i}")), on, &t)
                        .p(px(8.))
                        .pr(px(16.))
                        .gap(px(14.))
                        .child(thumb)
                        .child(match details.get(&path) {
                            Some((name, line)) => ui::list_text(name.clone(), line.clone()),
                            None => ui::list_text(name, short(&path)),
                        })
                        .child(tick(on, self.look.accent_soft))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if let Some(c) = &mut this.servers.create {
                                c.copy = Some(path.clone());
                            }
                            cx.notify();
                        })),
                );
            }
            list.into_any_element()
        };
        let world = section(
            tr("World").into(),
            None,
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(kinds)
                .child(detail),
        );

        // The game mode, three cards.
        let modes = div()
            .flex()
            .gap(px(10.))
            .children(MODES.iter().enumerate().map(|(i, &(id, label))| {
                choice(
                    SharedString::from(format!("create-mode-{id}")),
                    mode == id,
                    &t,
                )
                .flex_1()
                .p(px(16.))
                .child(choice_text(tr(label), tr(MODE_NOTES[i]).to_string()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(c) = &mut this.servers.create {
                        c.mode = id;
                    }
                    cx.notify();
                }))
            }));

        // The difficulty (a radio strip, 40 px options) and the players stepper.
        let strip = || {
            div()
                .flex()
                .gap(px(4.))
                .p(px(4.))
                .rounded(px(radius::LARGE))
                .bg(white(0.04))
        };
        let difficulties = strip().children(DIFFICULTIES.iter().map(|&(id, label)| {
            let on = difficulty == id;
            keyboard(
                div()
                    .id(SharedString::from(format!("create-difficulty-{id}")))
                    .flex()
                    .items_center()
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .font_family(horizon_theme::glass::FONT)
                    .text_size(px(14.))
                    .when(on, |d| {
                        d.bg(white(fill::BUTTON_HOVER))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(white(ink::FULL))
                    })
                    .when(!on, |d| {
                        d.text_color(white(ink::MUTED))
                            .hover(|s| s.text_color(white(ink::FULL)))
                    })
                    .child(tr(label)),
                &t,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some(c) = &mut this.servers.create {
                    c.difficulty = id;
                }
                cx.notify();
            }))
        }));
        let step = |id: &'static str, icon: &'static str, delta: i32, enabled: bool| {
            let b = keyboard(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(40.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::BUTTON)))
                    .child(ui::icon(icon, 17., white(ink::FULL))),
                &t,
            );
            if enabled {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(c) = &mut this.servers.create {
                        c.max_players = (c.max_players as i32 + delta).clamp(1, 100) as u32;
                    }
                    cx.notify();
                }))
            } else {
                b.opacity(0.35).cursor_default()
            }
        };
        let players = strip()
            .items_center()
            .child(step("create-fewer", "minus-sign", -1, max > 1))
            .child(
                ui::text(17., FontWeight::SEMIBOLD, ink::FULL)
                    .w(px(48.))
                    .flex()
                    .justify_center()
                    .child(max.to_string()),
            )
            .child(step("create-more", "plus-sign", 1, max < 100));
        let numbers = div()
            .flex()
            .justify_between()
            .gap(px(36.))
            .child(section(
                tr("Difficulty").into(),
                None,
                div().flex().child(difficulties),
            ))
            .child(section(
                tr("Most players at once").into(),
                None,
                div().flex().child(players),
            ));

        // Who may join: friends only (their names, the first an operator) or anyone.
        let mut who = div().flex().flex_col().child(
            div()
                .flex()
                .gap(px(10.))
                .child(
                    card(
                        "create-friends",
                        friends_only,
                        tr("Friends only").into(),
                        tr("A whitelist of the names below").to_string(),
                    )
                    .on_click(set(cx, |c| c.friends_only = true)),
                )
                .child(
                    card(
                        "create-anyone",
                        !friends_only,
                        tr("Anyone with the address").into(),
                        tr("Keep it on your network or Tailscale").to_string(),
                    )
                    .on_click(set(cx, |c| c.friends_only = false)),
                ),
        );
        if friends_only {
            who = who.child(self.create_friends(&friends, cx)).child(
                ui::card_note(tr(
                    "The first one becomes an operator. Add more later on the Players tab.",
                ))
                .mt(px(10.)),
            );
        }

        // Account checks (not in the mockup's words: Horizon players sign in with Microsoft now).
        let accounts = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(24.))
            .p(px(16.))
            .rounded(px(radius::POPOVER))
            .bg(white(0.04))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(ui::text(15., FontWeight::SEMIBOLD, ink::FULL).child(tr("Check Minecraft accounts")))
                    .child(
                        ui::text(13., FontWeight::NORMAL, ink::QUIET)
                            .mt(px(2.))
                            .max_w(px(560.))
                            .line_height(px(19.5))
                            .child(tr("On: every player signs in with Microsoft, so nobody can join under someone else's name. Turn it off only if a player can't sign in.")),
                    ),
            )
            .child(
                ui::toggle("create-accounts", check, self.look.accent, self.look.on_accent, &t)
                    .on_click(set(cx, |c| c.check_accounts = !c.check_accounts)),
            );

        let body = div()
            .id("create-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(36.))
            .px(px(36.))
            .pb(px(32.))
            .overflow_y_scroll()
            .child(section(
                tr("Name").into(),
                None,
                self.field(Field::Name, "My server", 400., cx).w_full(),
            ))
            .child(world)
            .child(section(tr("Game mode").into(), None, modes))
            .child(numbers)
            .child(section(tr("Who can join").into(), None, who))
            .child(accounts);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(head)
            .child(self.servers.create_scroll.wrap(body, &t).flex_1().min_h_0())
            .child(self.create_foot(cx))
    }

    /// The friends let in, as chips with their face (the first is an operator), then a field to add one.
    fn create_friends(&mut self, friends: &[String], cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let own = self
            .accounts
            .selected()
            .map(|a| (a.name.clone(), a.uuid.clone()));
        let mut chips = div().mt(px(12.)).flex().flex_wrap().gap(px(8.));
        for (i, name) in friends.iter().enumerate() {
            let face = match &own {
                Some((n, uuid)) if n == name => self.head(uuid, 30.).rounded(px(8.)),
                _ => div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(30.))
                    .rounded(px(8.))
                    .bg(white(fill::BUTTON))
                    .child(ui::icon("user", 16., white(ink::BODY))),
            };
            let mut chip = div()
                .flex()
                .items_center()
                .h(px(44.))
                .rounded(px(radius::ROW))
                .bg(white(fill::ROW_HOVER))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .pl(px(6.))
                        .pr(px(12.))
                        .child(face)
                        .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(name.clone()))
                        .when(i == 0, |d| {
                            d.child(
                                ui::text(12., FontWeight::NORMAL, ink::FULL)
                                    .text_color(ui::color(self.look.accent_soft))
                                    .child(tr("Operator")),
                            )
                        }),
                );
            if i > 0 {
                let gone = name.clone();
                chip = chip.child(
                    ui::icon_button(
                        SharedString::from(format!("create-friend-remove-{i}")),
                        "cancel-01",
                        28.,
                        14.,
                        8.,
                        false,
                        &t,
                    )
                    .mr(px(6.))
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            if let Some(c) = &mut this.servers.create {
                                c.friends.retain(|f| f != &gone);
                            }
                            cx.notify();
                        },
                    )),
                );
            }
            chips = chips.child(chip);
        }
        let typed = self.servers.value(Field::Players).trim().to_string();
        let ok = valid_name(&typed) && !friends.contains(&typed);
        let focused = self.servers.focused == Some(Field::Players);
        let add = keyboard(
            div()
                .id("create-friend-add")
                .flex()
                .items_center()
                .h(px(32.))
                .px(px(12.))
                .rounded(px(9.))
                .bg(white(fill::BUTTON))
                .cursor_pointer()
                .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
                .font_family(horizon_theme::glass::FONT)
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(white(ink::FULL))
                .child(tr("Add")),
            &t,
        );
        let add = if ok {
            add.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.add_create_friend();
                cx.notify();
            }))
        } else {
            add.opacity(0.4).cursor_default()
        };
        chips.child(
            div()
                .flex()
                .items_center()
                .h(px(44.))
                .pr(px(6.))
                .rounded(px(radius::ROW))
                .bg(white(if focused { fill::BUTTON } else { 0.04 }))
                .child(self.field(Field::Players, "Add a friend's name", 170., cx))
                .child(add),
        )
    }

    /// The name typed in the friend field joins the list, if Minecraft would take it.
    pub(super) fn add_create_friend(&mut self) {
        let typed = self.servers.value(Field::Players).trim().to_string();
        let Some(c) = &mut self.servers.create else {
            return;
        };
        if valid_name(&typed) && !c.friends.contains(&typed) {
            c.friends.push(typed);
            self.servers.text(Field::Players).set(String::new());
        }
    }

    /// The foot: the EULA and its link, what is happening, Cancel and Create server.
    fn create_foot(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let Some(c) = self.servers.create.as_ref() else {
            return div();
        };
        let (eula, missing, creating) = (c.eula, c.eula_missing, self.servers.creating);
        let look = self.look;
        let tickbox = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(22.))
            .rounded(px(7.))
            .bg(if eula {
                ui::color(look.accent)
            } else {
                white(fill::SELECTED)
            })
            .when(eula, |d| {
                d.child(ui::icon("tick-02", 15., ui::color(look.on_accent)))
            });
        let agree = keyboard(
            div()
                .id("create-eula")
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .child(tickbox)
                .child(
                    ui::text(14., FontWeight::NORMAL, ink::FULL)
                        .when(missing, |d| d.text_color(ui::danger()))
                        .child(tr("I agree to the Minecraft EULA")),
                ),
            &t,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            if let Some(c) = &mut this.servers.create {
                c.eula = !c.eula;
                c.eula_missing = false;
            }
            cx.notify();
        }));
        let read = div()
            .id("create-eula-read")
            .font_family(horizon_theme::glass::FONT)
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(ui::color(look.accent_soft))
            .cursor_pointer()
            .hover(|s| s.text_color(white(ink::FULL)))
            .child(tr("Read it"))
            .on_click(|_, _, cx| cx.open_url(servers::EULA_URL));
        // What is happening, between the EULA and the buttons (not in the mockup).
        let status = if creating {
            let last = self.servers.job_log.back().cloned().unwrap_or_default();
            Some(ui::card_note(format!("{} {last}", tr("Creating…"))).truncate())
        } else if let Some(p) = &self.servers.problem {
            Some(
                ui::text(13., FontWeight::NORMAL, ink::FULL)
                    .text_color(ui::danger())
                    .child(p.message.clone()),
            )
        } else if missing {
            Some(
                ui::text(13., FontWeight::NORMAL, ink::FULL)
                    .text_color(ui::danger())
                    .child(tr("Tick the EULA box to create the server.")),
            )
        } else {
            None
        };
        let fix = self.servers.problem.as_ref().and_then(|p| match &p.fix {
            Some(fix @ Fix::Prepare) => Some(fix.clone()),
            _ => None,
        });
        let cancel = keyboard(
            div()
                .id("create-cancel")
                .flex()
                .items_center()
                .h(px(48.))
                .px(px(20.))
                .rounded(px(radius::ROW))
                .cursor_pointer()
                .font_family(horizon_theme::glass::FONT)
                .text_size(px(15.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(white(ink::BODY))
                .hover(|s| s.bg(white(fill::ROW_HOVER)).text_color(white(ink::FULL)))
                .child(tr("Cancel")),
            &t,
        );
        let accent = ui::color(look.accent);
        let lit = gpui::hsla(accent.h, accent.s, (accent.l * 1.1).min(1.), 1.);
        let create = keyboard(
            div()
                .id("create-go")
                .flex()
                .items_center()
                .justify_center()
                .h(px(48.))
                .px(px(32.))
                .rounded(px(radius::LARGE))
                .bg(accent)
                .cursor_pointer()
                .hover(move |s| s.bg(lit))
                .font_family(horizon_theme::glass::FONT)
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::color(look.on_accent))
                // The mockup greys it until the EULA is ticked; a press then says why.
                .when(!eula, |d| d.opacity(0.5))
                .child(tr("Create server")),
            &t,
        );
        let buttons = if creating {
            div()
                .flex()
                .gap(px(8.))
                .opacity(0.5)
                .child(cancel)
                .child(create)
        } else {
            div()
                .flex()
                .gap(px(8.))
                .children(fix.map(|fix| {
                    ui::quiet("create-fix", tr("Prepare the game files"), 48., &t).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.fix(fix.clone(), cx);
                            cx.notify();
                        }),
                    )
                }))
                .child(cancel.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.servers.create = None;
                    this.servers.problem = None;
                    cx.notify();
                })))
                .child(create.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.create_server();
                    cx.notify();
                })))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .px(px(36.))
            .py(px(20.))
            .rounded_br(px(radius::MODAL))
            .bg(white(0.03))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_w_0()
                    .child(agree)
                    .child(read)
                    .children(status.map(|s| s.flex_1().min_w_0())),
            )
            .child(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn friend_names_follow_minecrafts_rules() {
        assert!(valid_name("Player46008"));
        assert!(valid_name("oak_leaf"));
        assert!(!valid_name("ab"));
        assert!(!valid_name("seventeen_letters"));
        assert!(!valid_name("no spaces"));
        assert!(!valid_name("dash-name"));
    }
}
