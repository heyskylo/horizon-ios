//! Settings' Language row (`horizon_launcher::i18n`): a dropdown of the system's language and the
//! ones the launcher has, each named in itself. A list rather than a segment per language: nine
//! segments took the row's whole width and left the row's own sentence a character wide.
use super::*;
use crate::glass_ui as ui;
use horizon_launcher::i18n::Language;

/// What the dropdown shows for a choice: the system's, in the launcher's language, or a
/// language's name in itself (never through the table: a table entry for "English" would rename
/// the very entry a player looks for).
fn choice_name(choice: Option<Language>) -> &'static str {
    match choice {
        None => tr("System"),
        Some(language) => language.name(),
    }
}

impl Launcher {
    fn pick_language(&mut self, choice: Option<Language>, cx: &mut Context<Self>) {
        self.settings.language = choice;
        self.save_settings();
        i18n::set(self.settings.language());
        self.language_menu = false;
        cx.notify();
    }

    /// The glass look's list: a popover like the others, its rows scrolling past ten.
    fn glass_language_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let mut rows = div()
            .id("language-list")
            .flex()
            .flex_col()
            .max_h(px(360.))
            .overflow_y_scroll();
        for choice in std::iter::once(None).chain(Language::ALL.map(Some)) {
            let id = choice.map_or("system", Language::code);
            rows = rows.child(
                ui::pick_row(
                    format!("language-pick-{id}"),
                    choice_name(choice),
                    choice == self.settings.language,
                    &t,
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_language(choice, cx)),
                ),
            );
        }
        let menu = ui::popover(220.)
            .id("language-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.language_menu = false;
                this.menu_closed_at = Some(event.position);
                cx.notify();
            }))
            .child(rows);
        // Anchored by its top right under the picker; it opens upwards instead when the window
        // has no room below for it.
        gpui::deferred(
            div()
                .absolute()
                .top(px(theme::CONTROL + 6.))
                .right_0()
                .child(gpui::anchored().anchor(gpui::Anchor::TopRight).child(menu)),
        )
        .with_priority(1)
    }

    pub(super) fn language_row(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut wrap = div().relative().child(
            keyboard(div().id("language-picker"), &t)
                .relative()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(theme::CONTROL))
                .px(px(14.))
                .border_2()
                .border_color(t.outline)
                .bg(t.button)
                .children(blocks::edges(t.button_bevel, t.lip, theme::LIP))
                .cursor_pointer()
                .hover(move |s| s.bg(t.raised))
                .child(
                    div()
                        .flex_1()
                        .font_family(theme::PIXEL)
                        .text_size(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(choice_name(self.settings.language)),
                )
                .child(blocks::icon("chevron-down", 16., t.muted))
                .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                    if this.closed_by_this_press(event) {
                        return;
                    }
                    this.language_menu = !this.language_menu;
                    cx.notify();
                })),
        );
        if self.language_menu && t.glass {
            wrap = wrap.child(self.glass_language_menu(cx));
        } else if self.language_menu {
            let mut menu = div()
                .id("language-menu")
                .absolute()
                .top(px(theme::CONTROL + 4.))
                .right_0()
                .w(px(220.))
                .p(px(6.))
                .border_2()
                .border_color(t.outline)
                .bg(t.surface)
                .flex()
                .flex_col()
                .occlude();
            for choice in std::iter::once(None).chain(Language::ALL.map(Some)) {
                let chosen = choice == self.settings.language;
                let id = choice.map_or("system", Language::code);
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("language-pick-{id}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(theme::RADIUS))
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.plate_soft))
                        .child(
                            text(theme::LABEL)
                                .text_color(t.ink)
                                .child(choice_name(choice)),
                        )
                        .when(chosen, |d| {
                            d.child(div().size(px(6.)).rounded_full().bg(t.accent))
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.pick_language(choice, cx)
                        })),
                );
            }
            wrap = wrap.child(gpui::deferred(menu).with_priority(1));
        }
        // The sentence is about following the computer's language, so it says nothing true once
        // the player has picked one: the row keeps it only while the choice is the system's.
        let said = self.settings.language.is_none().then(|| {
            div()
                .child(trf(
                    "System follows your computer's language: {language} here. A string not \
                     translated yet shows in English.",
                    &[("language", Language::system().name())],
                ))
                .into_any_element()
        });
        index_row(0, tr("Language"), said, Some(wrap.into_any_element()), &t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_launcher::i18n::{current, tr_in};

    #[test]
    fn a_language_is_named_in_itself_and_the_system_choice_in_the_launchers_language() {
        assert_eq!(choice_name(Some(Language::Russian)), "Русский");
        assert_eq!(choice_name(Some(Language::English)), "English");
        assert_eq!(choice_name(None), tr_in(current(), "System"));
    }
}
