//! The Wardrobe page's purchase history: the account's orders through Paddle (the merchant of
//! record), newest first. Mojang's guidelines ask for it. The shop itself is on the website.
use super::*;
use horizon_launcher::wardrobe::{Me, Purchase};

/// A purchase's state, in words.
fn purchase_state(p: &Purchase) -> &'static str {
    match p.status.as_str() {
        "refunded" => tr("Refunded"),
        "partly_refunded" => tr("Partly refunded"),
        "charged_back" => tr("Charged back"),
        _ => tr("Paid"),
    }
}

/// Whether a purchase still stands: a green chip, else an outline.
fn purchase_live(p: &Purchase) -> bool {
    matches!(p.status.as_str(), "paid" | "partly_refunded")
}

/// A small label: 24 px, round at 8, 12 px medium; green when `live`, else an outline.
fn chip(live: bool, label: impl Into<SharedString>) -> Div {
    let d = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(24.))
        .px(px(9.))
        .rounded(px(8.))
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap();
    let d = if live {
        d.bg(ui::live().opacity(0.16)).text_color(if ui::light() {
            ui::live()
        } else {
            ui::color(0xb8f5cf)
        })
    } else {
        d.border_1()
            .border_color(ui::white(0.16))
            .text_color(ui::white(ink::BODY))
    };
    d.child(label.into())
}

impl Launcher {
    /// The account's orders, newest first: date, item, price, state, order number. Nothing when
    /// there are none.
    pub(super) fn purchase_history(&self, c: Option<&Catalogue>, me: &Me) -> Option<Div> {
        if me.purchases.is_empty() {
            return None;
        }
        let lang = game_language();
        let rows = me.purchases.iter().map(|p| {
            let item = if p.item == "founder_set" {
                tr("The founder set").to_string()
            } else {
                c.and_then(|c| c.items.iter().find(|e| e.id == p.item))
                    .map_or_else(|| p.item.clone(), |e| e.name(lang).to_string())
            };
            let price = p.price().unwrap_or_else(|| tr("Free").to_string());
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .py(px(10.))
                .child(ui::text(13., FontWeight::NORMAL, ink::NOTE).child(day(p.at)))
                .child(
                    ui::text(14., FontWeight::MEDIUM, ink::FULL)
                        .flex_1()
                        .child(item),
                )
                .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(price))
                .child(chip(purchase_live(p), purchase_state(p)))
                .child(
                    ui::text(12., FontWeight::NORMAL, ink::NOTE)
                        .child(trf("Order {id}", &[("id", &p.transaction_id)])),
                )
        });
        Some(
            glass::plate(radius::PANEL)
                .p(px(24.))
                .flex()
                .flex_col()
                .child(ui::text(17., FontWeight::SEMIBOLD, ink::FULL).child(tr("Your purchases")))
                .children(rows),
        )
    }
}
