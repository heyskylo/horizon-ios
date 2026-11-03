//! Settings' Account card in the glass look, as the mockup's: who plays now (a big head, the
//! name, how), the offline names as chips with a field to add one, and the Microsoft card
//! beside them with its white Sign in with Microsoft button.
//!
//! Not in the mockup, kept in its patterns: the Microsoft accounts already signed in (in the
//! Microsoft card, with Play as, Refresh and Sign out), the name the game made up (a chip with
//! New name), each offline name's skin (a pencil in its chip) and the skin panel, the sign-in's
//! code, progress and problems.

use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use crate::servers_page::Field;
use account::Player;
use horizon_theme::glass::{fill, ink, radius};

impl Launcher {
    pub(super) fn glass_account(&mut self, cx: &mut Context<Self>) -> Div {
        let left = self.glass_who_plays(cx);
        let right = self.glass_microsoft_card(cx);
        div()
            .flex()
            .gap(px(8.))
            .child(left.flex_1().min_w_0().p(px(24.)))
            .child(right.flex_none().w(px(400.)))
    }

    /// The player now, the offline names and the field to add one.
    fn glass_who_plays(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let now = self.accounts.player();
        let (uuid, name, status) = match &now {
            Player::Microsoft(uuid) => (
                Some(uuid.clone()),
                self.accounts
                    .accounts
                    .iter()
                    .find(|a| &a.uuid == uuid)
                    .map(|a| a.name.clone())
                    .unwrap_or_default(),
                (Tone::Live, tr("Microsoft account · online servers too")),
            ),
            Player::Offline(name) => (
                None,
                name.clone(),
                (Tone::Off, tr("Offline · single player and LAN worlds")),
            ),
            Player::GameName => (
                None,
                self.offline_name.clone().unwrap_or_else(|| "Player".into()),
                (Tone::Off, tr("Offline · single player and LAN worlds")),
            ),
        };
        let head = div()
            .flex_none()
            .size(px(80.))
            .rounded(px(20.))
            .overflow_hidden()
            .child(
                self.profile_heads
                    .draw(uuid.as_deref(), 80., self.scale, &t),
            );
        let header = div().flex().items_center().gap(px(20.)).child(head).child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(10.))
                .child(
                    div()
                        .font_family(horizon_theme::glass::DISPLAY_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(32.))
                        .line_height(px(32.))
                        .text_color(white(ink::FULL))
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(ui::dot(status.0).mt(px(7.)))
                        .child(
                            ui::text(14., FontWeight::NORMAL, 0.65)
                                .flex_1()
                                .min_w_0()
                                .line_height(px(21.))
                                .child(status.1),
                        ),
                ),
        );

        // The chips: each offline name, the game's own name, then the field.
        let mut chips = div().flex().flex_wrap().gap(px(8.)).mt(px(12.));
        for (i, offline) in self.accounts.offline.clone().into_iter().enumerate() {
            let plays = now == Player::Offline(offline.clone());
            let (who, skin, forget) = (offline.clone(), offline.clone(), offline.clone());
            chips = chips.child(
                self.name_chip(
                    SharedString::from(format!("offline-chip-{i}")),
                    offline,
                    plays,
                )
                .when(!plays, |d| {
                    d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.play_as(Player::Offline(who.clone()));
                        cx.notify();
                    }))
                })
                .child(self.chip_icon(
                    SharedString::from(format!("offline-skin-{i}")),
                    "pencil-edit-02",
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_skin(skin.clone());
                        cx.notify();
                    }),
                ))
                .when(!plays, |d| {
                    d.child(self.chip_icon(
                        SharedString::from(format!("offline-forget-{i}")),
                        "cancel-01",
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.forget_offline(&forget);
                            cx.notify();
                        }),
                    ))
                }),
            );
        }
        if let Some(made_up) = self.offline_name.clone() {
            let plays = now == Player::GameName;
            chips = chips.child(
                self.name_chip("game-name-chip", made_up, plays)
                    .when(!plays, |d| {
                        d.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.play_as(Player::GameName);
                            cx.notify();
                        }))
                    })
                    .child(self.chip_icon(
                        "game-name-new".into(),
                        "refresh",
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            match instances::new_player_name() {
                                Ok(name) => this.offline_name = Some(name),
                                Err(e) => this.sign_in_state = Some(SignInState::Done(Err(e))),
                            }
                            cx.notify();
                        }),
                    )),
            );
        }
        let add = div()
            .flex()
            .items_center()
            .h(px(44.))
            .pr(px(6.))
            .rounded(px(radius::ROW))
            .bg(white(0.05))
            .child(self.field(Field::OfflineName, tr("Add a name"), 150., cx))
            .child(
                ui::quiet("offline-add", tr("Add"), 32., &t)
                    .px(px(12.))
                    .text_size(px(13.))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.offline_name_enter();
                        cx.notify();
                    })),
            );
        chips = chips.child(add);
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(
                ui::text(13., FontWeight::MEDIUM, 0.6)
                    .mt(px(32.))
                    .child(tr("Offline names")),
            )
            .child(chips)
            .child(
                ui::text(13., FontWeight::NORMAL, 0.5)
                    .mt(px(12.))
                    .line_height(px(19.5))
                    .child(tr(
                        "3 to 16 letters, numbers or underscores. Switch names from the top bar \
                         too. Online servers don't take offline names.",
                    )),
            )
            .children(self.skin_panel(cx).map(|p| p.mt(px(16.))))
    }

    /// An offline name's chip: its head and name, a tick on the one playing; white at 14 % for
    /// that one, 5 % (8 % under the pointer) for the others.
    fn name_chip(&self, id: impl Into<SharedString>, name: String, plays: bool) -> Stateful<Div> {
        let t = self.theme;
        div()
            .id(id.into())
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(44.))
            .pl(px(6.))
            .pr(px(6.))
            .rounded(px(radius::ROW))
            .when(plays, |d| d.bg(white(0.14)))
            .when(!plays, |d| {
                d.bg(white(0.05))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(0.08)))
            })
            .child(
                div()
                    .flex_none()
                    .size(px(30.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(self.profile_heads.draw(None, 30., self.scale, &t)),
            )
            .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(name))
            .when(plays, |d| {
                d.child(ui::icon("tick-02", 16., ui::color(ui::look().accent_soft)))
            })
    }

    /// A 28 px button in a chip (`size-7 rounded-[8px]`), white at 45 %.
    fn chip_icon(
        &self,
        id: SharedString,
        icon: &'static str,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        keyboard(
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(white(fill::BUTTON)))
                .child(ui::icon(icon, 14., white(0.45))),
            &self.theme,
        )
        .on_click(on_click)
    }

    /// The Microsoft card: the accounts signed in here, then adding one (the mockup's card).
    fn glass_microsoft_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let has_accounts = !self.accounts.accounts.is_empty();
        let (rows, _) = if has_accounts {
            ui::flat_rows(|| self.microsoft_rows(cx))
        } else {
            (div(), 0)
        };
        let (title, detail, action) = self.sign_in_parts(cx);
        let configured = account::client_id().is_some();
        let idle = self.sign_in_state.is_none()
            || matches!(self.sign_in_state, Some(SignInState::Done(_)));
        let first = !has_accounts && self.sign_in_target.is_none();
        let heading = if first {
            tr("Play online with your Microsoft account").to_string()
        } else {
            title
        };
        let note: AnyElement = if first && configured && self.sign_in_state.is_none() {
            div()
                .child(tr(
                    "Sign in with the account that owns Minecraft: Java Edition. Your name and \
                     skin come along to online servers, like in other launchers.",
                ))
                .into_any_element()
        } else {
            detail.into_any_element()
        };
        // Adding an account is the mockup's white button; signing one in again, the code and
        // the waiting keep their own buttons.
        let button: Option<AnyElement> = if configured && idle && self.sign_in_target.is_none() {
            Some(
                keyboard(
                    div()
                        .id("glass-sign-in")
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(10.))
                        .h(px(48.))
                        .w_full()
                        .mt(px(24.))
                        .rounded(px(radius::ROW))
                        .bg(gpui::white())
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.9))
                        .font_family(horizon_theme::glass::FONT)
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(0x100a1f))
                        .child(microsoft::logo(18.))
                        .child(tr("Sign in with Microsoft")),
                    &t,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.start_sign_in();
                    cx.notify();
                }))
                .into_any_element(),
            )
        } else {
            action.map(|a| div().mt(px(20.)).child(a).into_any_element())
        };
        div()
            .flex()
            .flex_col()
            .rounded(px(16.))
            .bg(white(0.045))
            .p(px(24.))
            .when(has_accounts, |d| {
                d.child(rows.mx(px(-16.)).mt(px(-16.)).mb(px(16.)))
            })
            .child(microsoft::logo(36.))
            .child(
                div()
                    .mt(px(20.))
                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(22.))
                    .line_height(px(27.5))
                    .text_color(white(ink::FULL))
                    .child(heading),
            )
            .child(
                ui::text(14., FontWeight::NORMAL, 0.65)
                    .mt(px(8.))
                    .line_height(px(21.7))
                    .child(note),
            )
            .children(button)
            .when(configured, |d| {
                d.child(
                    ui::text(12., FontWeight::NORMAL, 0.45)
                        .mt(px(12.))
                        .line_height(px(18.))
                        .child(tr(
                            "Microsoft's own page opens in your browser. Horizon never sees your \
                             password.",
                        )),
                )
            })
    }
}
