//! The Shop page (top bar, beside the Wardrobe, shown under the same rule: `shop::in_top_bar`):
//! what the cosmetics service's catalogue sells, tried on the account's own skin, put in a basket
//! and paid on the website. Nothing here asks for attention: the tab has no badge, the page never
//! opens by itself, and a catalogue that doesn't load says so in one quiet line. Paying starts
//! Paddle's checkout on the service and opens playhorizon.dev/shop/pay in the browser
//! (`horizon_launcher::shop::pay_url`); no card is typed here. The founder set is claimed the
//! same way, or at once where Paddle doesn't sell (`Claim::here`).
//!
//! It reads the Wardrobe's state (`Wardrobe`: the catalogue, the item files, what the account owns
//! and wears); a visit refreshes it as a Wardrobe visit does. Prices, sets, collections and the
//! Supporter badge all come from the catalogue: an item it doesn't price isn't sold.
use super::*;
use horizon_launcher::shop::{self as rules, Basket, FOUNDER_SET};
use horizon_launcher::wardrobe::{words, Entry, Me, Order, Player, Set};
use std::collections::{BTreeMap, HashSet};

mod basket;

/// The Shop's own state: the basket and what the player chose on the page.
#[derive(Default)]
pub(crate) struct Shop {
    basket: Basket,
    currency: Option<String>,
    /// The card tried on: an item, a set, the Supporter badge or the founder set.
    selected: Option<String>,
    /// The Supporter badge's price before it goes in the basket, in cents.
    amount: Option<u32>,
    gift: bool,
    /// The gift's recipient, once found.
    friend: Option<Player>,
    /// Enter in the friend's field: look the name up on the next draw.
    pub(crate) find_soon: bool,
    busy: bool,
    said: Option<String>,
    /// The player in 3D, the Wardrobe's view (`viewer`) with a camera of its own.
    viewer: Option<gpui::Entity<Viewer>>,
    /// The outfit last handed to the view.
    shown: Option<Outfit>,
    /// The pointer is turning the view.
    dragging: bool,
}

impl Shop {
    /// The development preview's Shop (`wardrobe_preview`): the founder set tried on.
    pub(super) fn preview(&mut self) {
        self.selected = Some(FOUNDER_SET.into());
    }
}

/// What a card sells: a priced item or a priced set.
enum Offer<'a> {
    Item(&'a Entry),
    Set(&'a Set),
}

impl Offer<'_> {
    fn id(&self) -> &str {
        match self {
            Self::Item(e) => &e.id,
            Self::Set(s) => &s.id,
        }
    }

    fn collection(&self) -> Option<&str> {
        match self {
            Self::Item(e) => e.collection.as_deref(),
            Self::Set(s) => s.collection.as_deref(),
        }
    }
}

/// What the catalogue sells, sets first: priced items (the Supporter badge has its own card)
/// and priced sets.
fn offers(c: &Catalogue) -> Vec<Offer<'_>> {
    let badge = c.supporter.as_ref().map(|s| s.item.as_str());
    let items = (c.items.iter())
        .filter(|e| !e.prices.is_empty() && Some(e.id.as_str()) != badge)
        .map(Offer::Item);
    let sets = (c.sets.iter().filter(|s| !s.prices.is_empty())).map(Offer::Set);
    sets.chain(items).collect()
}

/// The item ids behind an id: a set's items, the founder set's, or the id itself.
fn unpack<'a>(id: &'a str, c: &'a Catalogue, me: &'a Me) -> Vec<&'a str> {
    if id == FOUNDER_SET {
        return me.claim.items.iter().map(String::as_str).collect();
    }
    match c.sets.iter().find(|s| s.id == id) {
        Some(s) => s.items.iter().map(String::as_str).collect(),
        None => vec![id],
    }
}

/// An id's name and line: a set's, the founder set's or an item's.
fn about(id: &str, c: &Catalogue, lang: &str) -> (String, Option<String>) {
    if id == FOUNDER_SET {
        let line = "For the players who were here first. Free to claim; once claimed, it's yours \
                    to keep.";
        return (tr("The founder set").into(), Some(tr(line).into()));
    }
    let set = c.sets.iter().find(|s| s.id == id);
    let item = c.items.iter().find(|e| e.id == id);
    let (names, lines) = match (set, item) {
        (Some(s), _) => (&s.names, &s.lines),
        (None, Some(e)) => (&e.names, &e.lines),
        _ => return (id.to_owned(), None),
    };
    let name = words(names, lang).map_or_else(|| id.to_owned(), str::to_owned);
    (name, words(lines, lang).map(str::to_owned))
}

impl Launcher {
    pub(crate) fn shop_page(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let header = page_header(
            "Shop",
            "Things to wear for your player, seen by everyone who plays on Horizon.",
            &t,
        );
        let page = div().flex().flex_col().gap(px(24.));
        if self.wardrobe.is_none() && self.wardrobe_account().is_none() {
            return page.child(header).child(self.wardrobe_sign_in(cx));
        }
        if std::mem::take(&mut self.wardrobe_reload) || self.wardrobe.is_none() {
            self.open_wardrobe(cx);
        }
        if std::mem::take(&mut self.shop.find_soon) {
            self.shop_find(cx);
        }
        let viewer = self.shop_dress(cx);
        let Some(w) = self.wardrobe.as_ref() else {
            return page.child(header);
        };
        let (me, c) = match (&w.saved.me, &w.saved.catalogue, &w.status) {
            (_, _, Status::Offline | Status::Failed(_)) => {
                return page.child(header).child(self.shop_failed(cx))
            }
            (Some(me), Some(c), _) => (me.clone(), c.clone()),
            // The first answer is on its way.
            _ => return page.child(header),
        };
        let offered = rules::currencies(&c);
        if !(self.shop.currency.as_ref()).is_some_and(|x| offered.contains(x)) {
            let locale = sys_locale::get_locale();
            self.shop.currency = rules::guess_currency(&offered, locale.as_deref());
        }
        let currency = self.shop.currency.clone().unwrap_or_default();
        let mut chips = div().flex().gap(px(4.));
        for code in offered.into_iter().filter(|_| c.currencies.len() > 1) {
            let on = code == currency;
            let kind = if on { Kind::Secondary } else { Kind::Ghost };
            chips = chips.child(
                button(format!("shop-currency-{code}"), code.clone(), kind, &t).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.shop.currency = Some(code.clone());
                        cx.notify();
                    }),
                ),
            );
        }
        let inset = if t.glass {
            40. - glass::CONTENT_INSET + 5.
        } else {
            4.
        };
        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(24.))
            .child(header.flex_1().min_w_0())
            .child(div().flex_none().pt(px(inset)).child(chips));
        let left = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .w(px(320.))
            .flex_none()
            .child(self.shop_try_on(&me, &c, viewer, cx))
            .children(self.shop_details(&me, &c, &currency, cx))
            .child(self.shop_basket(&me, &c, &currency, cx));
        let right = self.shop_cards(&me, &c, &currency, cx).flex_1().min_w_0();
        page.child(header).child(
            div()
                .flex()
                .items_start()
                .gap(px(16.))
                .child(left)
                .child(right),
        )
    }

    /// The catalogue didn't load: say so, with a Retry.
    fn shop_failed(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let retry = button("shop-retry", "Retry", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.wardrobe_reload = true;
                cx.notify();
            },
        ));
        glass::plate(radius::PANEL)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .p(px(16.))
            .child(ui::text(14., FontWeight::NORMAL, ink::BODY).child(tr("The shop didn't load.")))
            .child(retry)
    }

    /// What each slot shows now: what the account wears, the basket's items over it, then the
    /// chosen card's.
    fn shop_tried(&self, me: &Me, c: &Catalogue) -> BTreeMap<String, String> {
        let ids = self.shop.basket.ids(c);
        let items: Vec<&str> = (ids.iter().chain(&self.shop.selected))
            .flat_map(|id| unpack(id, c, me))
            .collect();
        let tried = items.into_iter().filter_map(|id| {
            let e = c.items.iter().find(|e| e.id == id)?;
            Some((e.slot.as_str(), e.id.as_str()))
        });
        rules::try_on(&me.equipped, tried)
    }

    /// The player in 3D wearing what is tried on, handed to the view when that changed (after
    /// this frame, as the Wardrobe does: the page never reads the view while it renders).
    fn shop_dress(&mut self, cx: &mut Context<Self>) -> Option<gpui::Entity<Viewer>> {
        let w = self.wardrobe.as_ref()?;
        let (me, c) = (w.saved.me.as_ref()?, w.saved.catalogue.as_ref()?);
        let worn = self.shop_tried(me, c);
        let (skin, slim) = w.skin.clone().unwrap_or_else(|| (Arc::new(plain()), false));
        let outfit = Outfit {
            skin,
            slim,
            items: (worn.values().filter_map(|id| w.items.get(id)))
                .filter(|i| i.slot != Slot::NameTag)
                .cloned()
                .collect(),
        };
        let viewer = (self.shop.viewer)
            .get_or_insert_with(|| cx.new(|_| Viewer::new()))
            .clone();
        if self.shop.shown.as_ref() != Some(&outfit) {
            self.shop.shown = Some(outfit.clone());
            let motion = crate::sky::motion() != crate::sky::Motion::Still;
            let to = viewer.clone();
            cx.defer(move |cx| to.update(cx, |v, cx| v.dress(outfit, motion, cx)));
        }
        Some(viewer)
    }

    /// Sets whether the pointer turns the Shop's view, for its hand.
    fn shop_drag(cx: &mut Context<Self>) -> impl Fn(bool, &mut App) + Clone + 'static {
        let page = cx.entity().downgrade();
        move |on, cx: &mut App| {
            let _ = page.update(cx, |this, cx| {
                if this.shop.dragging != on {
                    this.shop.dragging = on;
                    cx.notify();
                }
            });
        }
    }

    /// The player in 3D with the name tag tried on over the head; drag to turn, scroll to zoom.
    fn shop_try_on(
        &mut self,
        me: &Me,
        c: &Catalogue,
        viewer: Option<gpui::Entity<Viewer>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let scale = self.scale;
        let tag = self.shop_tried(me, c).remove("name_tag").and_then(|id| {
            let hash = c.items.iter().find(|e| e.id == id)?.hash.clone();
            let item = self.wardrobe.as_ref()?.items.get(&id)?.clone();
            item.tag.is_some().then_some((id, hash, item))
        });
        let line = (tag.as_ref().and_then(|(.., i)| i.tag))
            .map(|g| gpui::rgb(u32::from_be_bytes([0, g.line[0], g.line[1], g.line[2]])));
        let mark = tag.map(|(id, hash, i)| {
            (self.wardrobe_marks)
                .get_or_bake(&id, &hash, scale, || Self::wardrobe_mark(&i, scale))
                .clone()
        });
        let name_tag = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(3.))
            .bg(gpui::rgba(0x0000_0066))
            .child(
                div()
                    .font_family(theme::MONO)
                    .text_size(px(13.))
                    .text_color(gpui::white())
                    .child(me.name.clone()),
            )
            .children(mark.map(|m| gpui::img(m).size(px(16.))))
            .when_some(line, |d, c| d.border_b_2().border_color(c));
        let reset = button("shop-reset", "Reset view", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                if let Some(v) = this.shop.viewer.clone() {
                    v.update(cx, |v, cx| v.reset(cx));
                }
            },
        ));
        let controls = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                ui::text(12., FontWeight::NORMAL, ink::NOTE)
                    .child(tr("Drag to turn, scroll to zoom.")),
            )
            .child(reset);
        glass::plate(radius::PANEL)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .p(px(20.))
            .child(name_tag)
            .child(div().w(px(viewer::W)).h(px(viewer::H)).children(
                viewer.map(|v| viewer::frame(v, self.shop.dragging, Self::shop_drag(cx))),
            ))
            .child(controls)
    }

    /// A card: its name and, under it, its price or that it's owned; the chosen one outlined.
    fn shop_card(&self, id: &str, title: String, under: String, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let on = self.shop.selected.as_deref() == Some(id);
        let look = ui::look();
        let chosen = id.to_owned();
        div().child(
            keyboard(div().id(SharedString::from(format!("shop-card-{id}"))), &t)
                .flex()
                .flex_col()
                .gap(px(2.))
                .w(px(168.))
                .p(px(12.))
                .rounded(px(radius::LARGE))
                .cursor_pointer()
                .bg(ui::white(0.045))
                .when(on, |d| {
                    let accent = if ui::light() {
                        look.accent
                    } else {
                        look.accent_soft
                    };
                    d.border_2().border_color(ui::color(accent))
                })
                .when(!on, |d| {
                    d.border_1()
                        .border_color(ui::white(0.10))
                        .hover(|s| s.bg(ui::white(0.08)))
                })
                .child(ui::text(14., FontWeight::MEDIUM, ink::FULL).child(title))
                .child(ui::text(12., FontWeight::NORMAL, ink::NOTE).child(under))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let again = this.shop.selected.as_deref() == Some(chosen.as_str());
                    this.shop.selected = (!again).then(|| chosen.clone());
                    cx.notify();
                })),
        )
    }

    /// The founder set while it can be claimed (or is owned), then each collection's sets and
    /// items, then those of no collection, then the Supporter badge.
    fn shop_cards(&self, me: &Me, c: &Catalogue, currency: &str, cx: &mut Context<Self>) -> Div {
        let lang = game_language();
        let owned: HashSet<String> = me.owned.iter().cloned().collect();
        let price = |offer: &Offer| match offer {
            Offer::Item(e) if owned.contains(&e.id) => tr("Owned").to_string(),
            Offer::Item(e) => e
                .prices
                .get(currency)
                .map_or_else(String::new, |p| rules::format_price(*p, currency)),
            Offer::Set(s) => match rules::set_price(s, c, &owned, currency) {
                (_, mine) if mine == s.items.len() => tr("Owned").to_string(),
                (amount, _) => rules::format_price(amount, currency),
            },
        };
        let mut column = div().flex().flex_col().gap(px(16.));
        let section = |title: Option<String>, line: Option<String>| {
            glass::plate(radius::PANEL)
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(16.))
                .children(title.map(|s| ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(s)))
                .children(line.map(|s| ui::text(13., FontWeight::NORMAL, ink::NOTE).child(s)))
        };
        let founder = me.claim.open || me.claim.claimed == Some(true);
        if founder {
            let under = if me.claim.claimed == Some(true) {
                tr("Owned")
            } else {
                tr("Free for founders")
            };
            let card = self.shop_card(FOUNDER_SET, tr("The founder set").into(), under.into(), cx);
            column = column.child(section(None, None).child(card));
        }
        let offers = offers(c);
        let mut groups: Vec<(Option<&str>, Option<String>, Option<String>)> = (c.collections)
            .iter()
            .map(|g| {
                let name = words(&g.names, lang).map(str::to_owned);
                (
                    Some(g.id.as_str()),
                    name,
                    words(&g.lines, lang).map(str::to_owned),
                )
            })
            .collect();
        groups.push((None, None, None));
        let known: HashSet<&str> = c.collections.iter().map(|g| g.id.as_str()).collect();
        let mut any = founder;
        for (id, name, line) in groups {
            let inside = offers.iter().filter(|o| match (id, o.collection()) {
                (Some(g), Some(of)) => g == of,
                (None, of) => of.is_none_or(|of| !known.contains(of)),
                _ => false,
            });
            let mut cards = div().flex().flex_wrap().gap(px(8.));
            let mut count = 0;
            for offer in inside {
                let (title, _) = about(offer.id(), c, lang);
                let title = match offer {
                    Offer::Set(s) => {
                        let n = s.items.len().to_string();
                        format!("{title} · {}", trf("{n} items", &[("n", &n)]))
                    }
                    Offer::Item(_) => title,
                };
                cards = cards.child(self.shop_card(offer.id(), title, price(offer), cx));
                count += 1;
            }
            if count > 0 {
                column = column.child(section(name, line).child(cards));
                any = true;
            }
        }
        if let Some(s) = &c.supporter {
            let (title, line) = about(&s.item, c, lang);
            let under = if owned.contains(&s.item) {
                tr("Owned").to_string()
            } else {
                let min = s.min.get(currency).copied().unwrap_or(0);
                format!("{}+", rules::format_price(min, currency))
            };
            let card = self.shop_card(&s.item, title, under, cx);
            column = column.child(section(None, line).child(card));
            any = true;
        }
        if !any {
            let none = ui::text(14., FontWeight::NORMAL, ink::BODY)
                .child(tr("Nothing is on sale right now."));
            column = column.child(section(None, None).child(none));
        }
        column
    }

    /// The chosen card: its name, its line, and what can be done with it.
    fn shop_details(
        &self,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let id = self.shop.selected.clone()?;
        let (name, line) = about(&id, c, game_language());
        let act = if id == FOUNDER_SET {
            self.shop_founder(me, cx)
        } else {
            self.shop_buy(&id, me, c, currency, cx)
        };
        let toast = (self.wardrobe.as_ref().and_then(|w| w.toast.as_ref()))
            .filter(|(_, at)| at.elapsed() < TOAST)
            .map(|(s, _)| s.clone());
        Some(
            glass::plate(radius::PANEL)
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(16.))
                .child(ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(name))
                .children(line.map(|l| ui::text(13., FontWeight::NORMAL, ink::BODY).child(l)))
                .children(act)
                .children(
                    (self.shop.said.clone().or(toast))
                        .map(|s| ui::text(13., FontWeight::NORMAL, ink::NOTE).child(s)),
                ),
        )
    }

    /// The founder set: owned, or claimed here with one click where Paddle doesn't sell, else
    /// through Paddle's free checkout in the browser.
    fn shop_founder(&self, me: &Me, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        if me.claim.claimed == Some(true) {
            return Some(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(tr("Owned")));
        }
        if !me.claim.open {
            return None;
        }
        let busy = self.shop.busy || self.wardrobe.as_ref().is_some_and(|w| w.busy);
        let label = match (busy, me.claim.here()) {
            (true, _) => "Claiming…",
            (false, true) => "Claim for free",
            (false, false) => "Claim the founder set",
        };
        let claim = button("shop-claim", label, Kind::Primary, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop_claim(cx);
                cx.notify();
            },
        ));
        Some(div().flex().child(claim))
    }

    fn shop_claim(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.wardrobe.as_ref().and_then(|w| w.saved.me.clone()) else {
            return;
        };
        if me.claim.here() {
            self.wardrobe_claim(cx);
            return;
        }
        let order = Order {
            name: me.name,
            items: vec![FOUNDER_SET.to_owned()],
            currency: "USD".into(),
            amount: None,
            gift: false,
        };
        let done = "Finish the free checkout in your browser. The set lands here a moment later.";
        self.shop_checkout(order, me.uuid, tr(done).into(), cx);
    }

    /// Starts Paddle's checkout on the service for `order` (for the account `uuid`), then opens
    /// the website's pay page in the browser and says `done`.
    fn shop_checkout(&mut self, order: Order, uuid: String, done: String, cx: &mut Context<Self>) {
        if self.shop.busy {
            return;
        }
        self.shop.busy = true;
        self.shop.said = None;
        let task =
            (cx.background_executor()).spawn(async move { Service::live().checkout(&order) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.shop.busy = false;
                let url = match result {
                    Ok(c) if c.uuid == uuid => {
                        rules::pay_url(&c.transaction_id).ok_or(Error::Service(200))
                    }
                    Ok(_) => Err(Error::Service(200)),
                    Err(e) => Err(e),
                };
                this.shop.said = Some(match url {
                    Ok(url) => {
                        cx.open_url(&url);
                        done
                    }
                    Err(e) => e.message(),
                });
                cx.notify();
            });
        })
        .detach();
    }
}
