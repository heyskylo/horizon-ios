//! Settings' Account card in the glass look, as the mockup's: who plays now (a big head, the
//! name, how), and the Microsoft card beside it with its white Sign in with Microsoft button.
//!
//! Not in the mockup, kept in its patterns: the Microsoft accounts already signed in (in the
//! Microsoft card, with Play as, Refresh and Sign out), the sign-in's code, progress and problems.

use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use horizon_theme::glass::{ink, radius};

impl Launcher {
    pub(super) fn glass_account(&mut self, cx: &mut Context<Self>) -> Div {
        let wide = self.window_width >= crate::glass_home::WIDE;
        let left = self.glass_who_plays();
        let right = self.glass_microsoft_card(cx);
        if wide {
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(left.flex_1().min_w(px(280.)).p(px(20.)))
                .child(right.flex_none().w(px(360.)))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(left.w_full().min_w_0().p(px(20.)))
                .child(right.w_full())
        }
    }

    /// The player now.
    fn glass_who_plays(&self) -> Div {
        let t = self.theme;
        let (uuid, name, status) = match self.accounts.selected() {
            Some(a) => (
                Some(a.uuid.clone()),
                a.name.clone(),
                (Tone::Live, tr("Microsoft account · online servers too")),
            ),
            None => (
                None,
                tr("Sign in with Microsoft").to_string(),
                (Tone::Off, tr(account::NEEDS_OWNER)),
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

        div().flex().flex_col().child(header)
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
                        // On a light plate, Microsoft's white button keeps an edge.
                        .when(ui::light(), |d| d.border_1().border_color(ui::white(0.15)))
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
