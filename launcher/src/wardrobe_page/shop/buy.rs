//! Buying the open offer: Buy, for the account, and Gift, for a friend found by their Minecraft
//! name (what they own already drops out of the price, as the service counts it). Either starts
//! Paddle's checkout on the service and opens the website's pay page in the browser. "Ask
//! whoever pays before you buy. Refunds within 14 days for any reason." always sits under the
//! buttons. The Supporter badge's price is chosen here, in steps.
use super::*;
use crate::servers_page::Field;

/// One step of the Supporter badge's price, in cents.
const STEP: u32 = 100;

impl Launcher {
    /// What the open offer costs and its buttons: Buy (not for what the account owns) and Gift.
    pub(super) fn shop_buy(
        &mut self,
        id: &str,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let badge = c.supporter.clone().filter(|s| s.item == id);
        let mine: HashSet<String> = me.owned.iter().cloned().collect();
        let owned = rules::owns(id, c, &mine);
        let ready = !self.shop.busy;
        let mut out = div().flex().flex_col().gap(px(12.)).pt(px(6.));
        if let Some(s) = &badge {
            let min = s.min.get(currency).copied().unwrap_or(0);
            let max = s.max.get(currency).copied().unwrap_or(min).max(min);
            let amount = self.shop.amount.unwrap_or(min).clamp(min, max);
            self.shop.amount = Some(amount);
            let step = |id: &'static str, label: &'static str, by: i64| {
                button(id, label, Kind::Ghost, &t).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        let next = (i64::from(amount) + by).clamp(min.into(), max.into());
                        this.shop.amount = Some(next as u32);
                        cx.notify();
                    },
                ))
            };
            out = out.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(step("shop-less", "−", -i64::from(STEP)))
                    .child(
                        ui::text(24., FontWeight::SEMIBOLD, ink::FULL)
                            .child(rules::format_price(amount, currency)),
                    )
                    .child(step("shop-more", "+", i64::from(STEP))),
            );
        }
        let gift = self.shop.gift;
        let chosen = id.to_owned();
        let buy = (!owned).then(|| {
            button("shop-buy", "Buy", Kind::Primary, &t)
                .when(!ready, |b| b.opacity(0.5))
                .when(ready, |b| {
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.shop.gift = false;
                        this.shop_pay(&chosen, false, cx);
                        cx.notify();
                    }))
                })
        });
        let gift_kind = if gift { Kind::Secondary } else { Kind::Ghost };
        let gift_button = button("shop-gift", "Gift", gift_kind, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop.gift = !this.shop.gift;
                this.shop.said = None;
                cx.notify();
            },
        ));
        out = out.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .children(buy)
                .child(gift_button),
        );
        if gift {
            out = out.child(self.shop_gift(id, c, currency, cx));
        }
        let ask = "Ask whoever pays before you buy. Refunds within 14 days for any reason.";
        out = out.child(ui::text(12., FontWeight::NORMAL, ink::NOTE).child(tr(ask)));
        if currency == "USD" {
            let tax = "US sales tax is added where a state charges it.";
            out = out.child(ui::text(12., FontWeight::NORMAL, ink::NOTE).child(tr(tax)));
        }
        let toast = (self.wardrobe.as_ref().and_then(|w| w.toast.as_ref()))
            .filter(|(_, at)| at.elapsed() < TOAST)
            .map(|(s, _)| s.clone());
        out.children(
            (self.shop.said.clone().or(toast))
                .map(|s| ui::text(13., FontWeight::MEDIUM, ink::FULL).child(s)),
        )
    }

    /// The gift: the friend's Minecraft name, looked up, then Pay for them.
    fn shop_gift(
        &mut self,
        id: &str,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let out = div().flex().flex_col().gap(px(10.));
        let Some(friend) = self.shop.friend.clone() else {
            let field = self.field(
                Field::ShopGift,
                tr("Your friend's Minecraft name"),
                220.,
                cx,
            );
            let find = button("shop-find", "Find", Kind::Secondary, &t).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    this.shop.find_soon = true;
                    cx.notify();
                },
            ));
            return out.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field)
                    .child(find),
            );
        };
        let change = button("shop-friend-change", "Remove", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop.friend = None;
                cx.notify();
            },
        ));
        let theirs: HashSet<String> = friend.owned.iter().cloned().collect();
        let badge = c.supporter.as_ref().is_some_and(|s| s.item == id);
        let price = match (badge, self.shop.amount) {
            (true, Some(amount)) if !theirs.contains(id) => rules::format_price(amount, currency),
            _ => self.shop_price(id, c, &theirs, currency),
        };
        let row = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                ui::text(14., FontWeight::MEDIUM, ink::FULL)
                    .child(trf("For {name}", &[("name", &friend.name)])),
            )
            .child(change);
        if price == tr("Owned") {
            return out
                .child(row)
                .child(ui::text(13., FontWeight::NORMAL, ink::NOTE).child(price));
        }
        let ready = !self.shop.busy;
        let chosen = id.to_owned();
        let pay = button(
            "shop-pay",
            trf("Pay {price} in your browser", &[("price", &price)]),
            Kind::Primary,
            &t,
        )
        .when(!ready, |b| b.opacity(0.5))
        .when(ready, |b| {
            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.shop_pay(&chosen, true, cx);
                cx.notify();
            }))
        });
        out.child(row).child(div().flex().child(pay))
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

    /// Checks `id` out for the account, or for the friend found when `gift`.
    fn shop_pay(&mut self, id: &str, gift: bool, cx: &mut Context<Self>) {
        let Some(w) = self.wardrobe.as_ref() else {
            return;
        };
        let (Some(me), Some(c)) = (&w.saved.me, &w.saved.catalogue) else {
            return;
        };
        let (name, uuid) = match (gift, &self.shop.friend) {
            (false, _) => (me.name.clone(), me.uuid.clone()),
            (true, Some(f)) => (f.name.clone(), f.uuid.clone()),
            (true, None) => return,
        };
        let badge = c.supporter.as_ref().is_some_and(|s| s.item == id);
        let order = Order {
            name,
            items: vec![id.to_owned()],
            currency: self.shop.currency.clone().unwrap_or_else(|| "USD".into()),
            amount: if badge { self.shop.amount } else { None },
            gift,
        };
        let done = "Finish paying in your browser. Your items reach the Wardrobe a moment later.";
        self.shop_checkout(order, uuid, tr(done).into(), cx);
    }
}
