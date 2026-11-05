//! The Shop's basket: what the player picked, its lines and total in the chosen currency with the
//! service's rules (`horizon_launcher::shop::lines`), who gets it (the account, or a friend for a
//! gift), and Pay, which starts Paddle's checkout on the service and opens the website's pay page
//! in the browser. "Ask whoever pays before you buy. Refunds within 14 days for any reason." always
//! sits by the button. The checkout's total is what Paddle charges; this is only its preview.
use super::*;
use crate::servers_page::Field;
use horizon_launcher::shop::Dropped;

/// One step of the Supporter badge's price, in cents.
const STEP: u32 = 100;

/// Who gets the basket: the friend for a gift (None until found), else the account itself; their
/// name, UUID and what they own already.
fn recipient(shop: &Shop, me: &Me) -> Option<(String, String, HashSet<String>)> {
    let (name, uuid, owned) = match (shop.gift, &shop.friend) {
        (false, _) => (&me.name, &me.uuid, &me.owned),
        (true, Some(f)) => (&f.name, &f.uuid, &f.owned),
        (true, None) => return None,
    };
    Some((name.clone(), uuid.clone(), owned.iter().cloned().collect()))
}

impl Launcher {
    /// A sold item, set or the Supporter badge: what it costs whoever gets the basket, and Add to
    /// basket or Remove (nothing for what they own).
    pub(super) fn shop_buy(
        &self,
        id: &str,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let t = self.theme;
        let owned = recipient(&self.shop, me).map(|r| r.2).unwrap_or_default();
        let badge = c.supporter.as_ref().filter(|s| s.item == id);
        let (price, mine) = match (badge, c.sets.iter().find(|s| s.id == id)) {
            (Some(_), _) => (None, owned.contains(id)),
            (None, Some(s)) => {
                let (amount, n) = rules::set_price(s, c, &owned, currency);
                (Some(amount), n == s.items.len())
            }
            (None, None) => {
                let e = c.items.iter().find(|e| e.id == id)?;
                (Some(*e.prices.get(currency)?), owned.contains(id))
            }
        };
        if mine {
            return Some(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(tr("Owned")));
        }
        let inside = match badge {
            Some(_) => self.shop.basket.supporter.is_some(),
            None => self.shop.basket.items.iter().any(|i| i == id),
        };
        let label = if inside { "Remove" } else { "Add to basket" };
        let kind = if inside { Kind::Ghost } else { Kind::Secondary };
        let chosen = id.to_owned();
        let toggle = button("shop-add", label, kind, &t).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                let shop = &mut this.shop;
                if this
                    .wardrobe
                    .as_ref()
                    .and_then(|w| w.saved.catalogue.as_ref())
                    .is_some_and(|c| c.supporter.as_ref().is_some_and(|s| s.item == chosen))
                {
                    shop.basket.supporter = match shop.basket.supporter {
                        Some(_) => None,
                        None => Some(shop.amount.unwrap_or(0)),
                    };
                } else {
                    shop.basket.toggle(&chosen);
                }
                shop.said = None;
                cx.notify();
            },
        ));
        let row = div().flex().items_center().gap(px(12.));
        let row = match (price, badge) {
            (Some(p), _) => row.child(
                ui::text(15., FontWeight::MEDIUM, ink::FULL)
                    .child(rules::format_price(p, currency)),
            ),
            (None, Some(s)) => {
                let min = s.min.get(currency).copied().unwrap_or(0);
                let max = s.max.get(currency).copied().unwrap_or(min).max(min);
                let amount = self.supporter_amount(min, max);
                let step = |id: &'static str, label: &'static str, by: i64| {
                    button(id, label, Kind::Ghost, &t).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let next = (i64::from(amount) + by).clamp(min.into(), max.into());
                            let next = next as u32;
                            this.shop.amount = Some(next);
                            if this.shop.basket.supporter.is_some() {
                                this.shop.basket.supporter = Some(next);
                            }
                            cx.notify();
                        },
                    ))
                };
                row.child(step("shop-less", "−", -i64::from(STEP)))
                    .child(
                        ui::text(15., FontWeight::MEDIUM, ink::FULL)
                            .child(rules::format_price(amount, currency)),
                    )
                    .child(step("shop-more", "+", i64::from(STEP)))
            }
            (None, None) => row,
        };
        Some(row.child(toggle))
    }

    /// The Supporter badge's price as chosen, kept within what the shop takes in this currency.
    fn supporter_amount(&self, min: u32, max: u32) -> u32 {
        let chosen = self.shop.basket.supporter.or(self.shop.amount);
        chosen.unwrap_or(min).clamp(min, max)
    }

    pub(super) fn shop_basket(
        &mut self,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let lang = game_language();
        let plate = glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .child(ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(tr("Your basket")));
        // The badge's price follows the currency's range.
        if let (Some(s), Some(_)) = (&c.supporter, self.shop.basket.supporter) {
            let min = s.min.get(currency).copied().unwrap_or(0);
            let max = s.max.get(currency).copied().unwrap_or(min).max(min);
            self.shop.basket.supporter = Some(self.supporter_amount(min, max));
        }
        if self.shop.basket.is_empty() {
            return plate.child(
                ui::text(13., FontWeight::NORMAL, ink::NOTE).child(tr("Your basket is empty.")),
            );
        }
        let who = recipient(&self.shop, me);
        let owned = who.as_ref().map(|r| r.2.clone()).unwrap_or_default();
        let (lines, total) = rules::lines(&self.shop.basket, c, &owned, currency);
        let mut plate = plate;
        for line in lines {
            let (name, _) = about(&line.id, c, lang);
            let note = line.dropped.map(|d| match d {
                Dropped::Owned => tr("Owned"),
                Dropped::InSet => tr("In a set in your basket"),
            });
            let id = line.id.clone();
            let badge = c.supporter.as_ref().is_some_and(|s| s.item == id);
            let remove = button(format!("shop-remove-{id}"), "Remove", Kind::Ghost, &t).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if badge {
                        this.shop.basket.supporter = None;
                    } else {
                        this.shop.basket.toggle(&id);
                    }
                    cx.notify();
                }),
            );
            let ink_of = if line.dropped.is_some() {
                ink::FAINT
            } else {
                ink::FULL
            };
            plate = plate.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(ui::text(14., FontWeight::NORMAL, ink_of).child(name))
                            .children(
                                note.map(|n| ui::text(12., FontWeight::NORMAL, ink::NOTE).child(n)),
                            ),
                    )
                    .child(
                        ui::text(14., FontWeight::NORMAL, ink_of)
                            .child(rules::format_price(line.amount, currency)),
                    )
                    .child(remove),
            );
        }
        let price = rules::format_price(total, currency);
        let tax = if currency == "USD" {
            "US sales tax is added where a state charges it."
        } else {
            "VAT included where it applies."
        };
        let plate = plate
            .child(
                div()
                    .flex()
                    .justify_between()
                    .pt(px(8.))
                    .border_t_1()
                    .border_color(ui::white(0.10))
                    .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(tr("Total")))
                    .child(ui::text(18., FontWeight::SEMIBOLD, ink::FULL).child(price.clone())),
            )
            .child(ui::text(12., FontWeight::NORMAL, ink::NOTE).child(tr(tax)))
            .child(self.shop_gift(cx));
        let ready = who.is_some() && total > 0 && !self.shop.busy;
        let pay = button(
            "shop-pay",
            trf("Pay {price} in your browser", &[("price", &price)]),
            Kind::Primary,
            &t,
        )
        .when(!ready, |b| b.opacity(0.5))
        .when(ready, |b| {
            b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.shop_pay(cx);
                cx.notify();
            }))
        });
        let ask = "Ask whoever pays before you buy. Refunds within 14 days for any reason.";
        let opens = "Paddle's checkout opens in your browser. Nothing is charged in the launcher.";
        plate
            .child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(tr(ask)))
            .child(pay)
            .child(ui::text(12., FontWeight::NORMAL, ink::NOTE).child(tr(opens)))
    }

    /// This is a gift: the friend's Minecraft name, looked up, then whose account gets it.
    fn shop_gift(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let gift = self.shop.gift;
        let row = div()
            .id("shop-gift")
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .child(switch("shop-gift-switch", gift, &t))
            .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(tr("This is a gift")))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.shop.gift = !this.shop.gift;
                this.shop.said = None;
                cx.notify();
            }));
        let out = div().flex().flex_col().gap(px(8.)).child(row);
        if !gift {
            return out;
        }
        if let Some(friend) = self.shop.friend.clone() {
            let change = button("shop-friend-change", "Remove", Kind::Ghost, &t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.shop.friend = None;
                    cx.notify();
                }),
            );
            let name = trf("For {name}", &[("name", &friend.name)]);
            return out.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::text(14., FontWeight::NORMAL, ink::FULL).child(name))
                    .child(change),
            );
        }
        let field = self.field(
            Field::ShopGift,
            tr("Your friend's Minecraft name"),
            200.,
            cx,
        );
        let find = button("shop-find", "Find", Kind::Secondary, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop.find_soon = true;
                cx.notify();
            },
        ));
        out.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(field)
                .child(find),
        )
    }

    /// Looks the friend's name up: the account and what it owns, or why not.
    pub(super) fn shop_find(&mut self, cx: &mut Context<Self>) {
        let name = self.servers.value(Field::ShopGift).trim().to_owned();
        if name.is_empty() || self.shop.busy {
            return;
        }
        self.shop.busy = true;
        self.shop.said = None;
        let task = (cx.background_executor()).spawn(async move { Service::live().player(&name) });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.shop.busy = false;
                match found {
                    Ok(p) => this.shop.friend = Some(p),
                    Err(e) => this.shop.said = Some(e.message()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn shop_pay(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wardrobe.as_ref() else {
            return;
        };
        let (Some(me), Some(c)) = (&w.saved.me, &w.saved.catalogue) else {
            return;
        };
        let Some((name, uuid, _)) = recipient(&self.shop, me) else {
            return;
        };
        let order = Order {
            name,
            items: self.shop.basket.ids(c),
            currency: self.shop.currency.clone().unwrap_or_else(|| "USD".into()),
            amount: self.shop.basket.supporter,
            gift: self.shop.gift,
        };
        let done = "Finish paying in your browser. Your items reach the Wardrobe a moment later.";
        self.shop_checkout(order, uuid, tr(done).into(), cx);
    }
}
