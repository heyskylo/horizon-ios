//! The Shop page (top bar, beside the Wardrobe, shown under the same rule: `shop::in_top_bar`),
//! laid out as a game's item shop: the season's pack across the top, then rows of large tiles,
//! each the thing itself drawn (`thumbs`), its name and its price. A tile opens it big on the
//! account's own player in 3D (drag to turn), with its items for a look or the pack, Buy and
//! Gift. Nothing here asks for attention: the tab has no badge, the page never opens by itself,
//! and a catalogue that doesn't load says so in one quiet line. Buying starts Paddle's checkout on
//! the service and opens playhorizon.dev/shop/pay in the browser (`horizon_launcher::shop::pay_url`);
//! no card is typed here. The founder set is claimed the same way, or at once where Paddle
//! doesn't sell (`Claim::here`).
//!
//! It reads the Wardrobe's state (`Wardrobe`: the catalogue, the item files, what the account owns
//! and wears); a visit refreshes it as a Wardrobe visit does. Prices, sets, collections and the
//! Supporter badge all come from the catalogue: an item it doesn't price isn't sold.
use super::*;
use gpui::Hsla;
use horizon_cosmetics::viewer::BodyMotion;
use horizon_cosmetics::{Part, Pose, BODY};
use horizon_launcher::shop::{self as rules, FOUNDER_SET};
use horizon_launcher::wardrobe::{words, Me, Order, Player, Set};
use std::collections::{BTreeMap, HashSet};

mod buy;
mod thumbs;
pub(crate) use thumbs::Thumbs;
use thumbs::{Ask, Subject};

/// A tile: its size, and its picture's height, in points.
const TILE: (f32, f32) = (168., 228.);
const TILE_PICTURE: f32 = 160.;
/// A look's tile, larger: the whole player.
const LOOK: (f32, f32) = (356., 396.);
const LOOK_PICTURE: f32 = 336.;
/// The hero's picture.
const HERO: (f32, f32) = (560., 320.);
/// A set's item in its big view.
const PIECE: (f32, f32) = (104., 132.);
const PIECE_PICTURE: f32 = 92.;
/// The big view's player.
const VIEW: (f32, f32) = (360., 470.);
const GAP: f32 = 12.;

/// The Shop's own state: what is open and what the player chose there.
#[derive(Default)]
pub(crate) struct Shop {
    currency: Option<String>,
    /// The offer open big: an item, a set, the Supporter badge or the founder set.
    selected: Option<String>,
    /// The Supporter badge's price, in cents.
    amount: Option<u32>,
    /// Buying for a friend.
    gift: bool,
    /// The gift's recipient, once found.
    friend: Option<Player>,
    /// Enter in the friend's field: look the name up on the next draw.
    pub(crate) find_soon: bool,
    busy: bool,
    said: Option<String>,
    /// The player in 3D, the Wardrobe's view (`viewer`) with a camera of its own.
    viewer: Option<gpui::Entity<Viewer>>,
    /// The outfit last handed to the view, and the emote it plays.
    shown: Option<(Outfit, Option<String>)>,
    /// The pointer is turning the view.
    dragging: bool,
    /// The tile under the pointer, lifted.
    hover: Option<String>,
    thumbs: Thumbs,
}

impl Shop {
    /// The development preview's Shop (`wardrobe_preview`): `HORIZON_SHOP_OPEN=<id>` opens an
    /// offer big.
    pub(super) fn preview(&mut self) {
        self.selected = std::env::var("HORIZON_SHOP_OPEN").ok();
    }

    pub(crate) fn free_replaced(&mut self, window: &mut gpui::Window) {
        self.thumbs.free_replaced(window);
    }
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

/// The season's pack, the Shop's hero: the first collection's pack, else its largest set.
fn hero(c: &Catalogue) -> Option<&Set> {
    let sold = |s: &&Set| !s.prices.is_empty();
    let pack = (c.collections.iter().filter_map(|g| g.pack.as_deref()))
        .find_map(|id| c.sets.iter().filter(sold).find(|s| s.id == id));
    pack.or_else(|| c.sets.iter().filter(sold).max_by_key(|s| s.items.len()))
}

/// A tile's colour: one per kind of thing, so that a row reads at a glance.
fn tint(slot: &str) -> u32 {
    match slot {
        "head" => 0xf2a13b,
        "torso" => 0x3bb8c4,
        "back" => 0x7cc46a,
        "shoulder" | "companion" => 0xb07cff,
        "trail" | "aura" => 0xff7aa8,
        "emote" => 0x5b8cff,
        "name_tag" => 0xc9c4d8,
        // Sets and the pack.
        _ => horizon_theme::glass::ACCENT,
    }
}

/// A tile's ground: the page's colour nearly solid (the sky doesn't show through), `colour`
/// washing down from the top at `top`.
fn ground(colour: Hsla, top: f32) -> [Div; 2] {
    let solid = if ui::light() {
        gpui::white().opacity(0.86)
    } else {
        ui::color(horizon_theme::glass::PAGE).opacity(0.84)
    };
    [
        div().absolute().top_0().left_0().size_full().bg(solid),
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(colour.opacity(top), 0.),
                gpui::linear_color_stop(colour.opacity(0.05), 0.9),
            )),
    ]
}

/// An emote played in the big view, as the viewer takes it.
struct Played {
    item: Arc<Item>,
    rest: [Pose; 6],
}

impl Played {
    fn motion(item: Arc<Item>) -> Option<Arc<dyn BodyMotion>> {
        item.emote.as_ref()?;
        let body = horizon_cosmetics::preview::player_model(false);
        let rest = BODY.map(|b| {
            (body.children.iter().find(|p: &&Part| p.name == b.part())).map_or([0.0; 9], |p| p.pose)
        });
        Some(Arc::new(Self { item, rest }))
    }
}

impl BodyMotion for Played {
    fn length(&self) -> f32 {
        self.item.emote.as_ref().map_or(0., |e| e.length)
    }

    fn pose(&self, ticks: f32, set: &mut dyn FnMut(&str, Pose)) {
        let Some(e) = &self.item.emote else {
            return;
        };
        let mut poses = self.rest;
        e.pose_body(ticks, e.weight(ticks), &self.rest, &mut poses);
        for (bone, pose) in BODY.iter().zip(poses) {
            set(bone.part(), pose);
        }
    }
}

impl Launcher {
    pub(crate) fn shop_page(&mut self, cx: &mut Context<Self>) -> Div {
        let page = div().flex().flex_col().gap(px(28.));
        let title = ui::page_head(tr("Shop"), "").mb(px(0.));
        if self.wardrobe.is_none() && self.wardrobe_account().is_none() {
            return page.child(title).child(self.wardrobe_sign_in(cx));
        }
        if std::mem::take(&mut self.wardrobe_reload) || self.wardrobe.is_none() {
            self.open_wardrobe(cx);
        }
        if std::mem::take(&mut self.shop.find_soon) {
            self.shop_find(cx);
        }
        let viewer = self.shop_dress(cx);
        let Some(w) = self.wardrobe.as_ref() else {
            return page.child(title);
        };
        let (me, c) = match (&w.saved.me, &w.saved.catalogue, &w.status) {
            (_, _, Status::Offline | Status::Failed(_)) => {
                return page.child(title).child(self.shop_failed(cx))
            }
            (Some(me), Some(c), _) => (me.clone(), c.clone()),
            // The first answer is on its way.
            _ => return page.child(title),
        };
        let offered = rules::currencies(&c);
        if !(self.shop.currency.as_ref()).is_some_and(|x| offered.contains(x)) {
            let locale = sys_locale::get_locale();
            self.shop.currency = rules::guess_currency(&offered, locale.as_deref());
        }
        let currency = self.shop.currency.clone().unwrap_or_default();
        let head = div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(24.))
            .child(title)
            .child(self.shop_currencies(&c, &currency, cx));
        let body = match self.shop.selected.clone() {
            Some(id) => self.shop_open(&id, &me, &c, &currency, viewer, cx),
            None => self.shop_front(&me, &c, &currency, cx),
        };
        self.shop_draw_pictures(cx);
        page.child(head).child(body)
    }

    /// The currencies, when there is a choice.
    fn shop_currencies(&self, c: &Catalogue, currency: &str, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut chips = div().flex().gap(px(4.));
        for code in rules::currencies(c)
            .into_iter()
            .filter(|_| c.currencies.len() > 1)
        {
            let kind = if code == currency {
                Kind::Secondary
            } else {
                Kind::Ghost
            };
            chips = chips.child(
                button(format!("shop-currency-{code}"), code.clone(), kind, &t).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.shop.currency = Some(code.clone());
                        cx.notify();
                    }),
                ),
            );
        }
        chips
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

    /// The item files behind an id, in the catalogue's order of slots.
    fn shop_items(&self, id: &str, c: &Catalogue, me: &Me) -> Vec<Arc<Item>> {
        let Some(w) = self.wardrobe.as_ref() else {
            return Vec::new();
        };
        unpack(id, c, me)
            .into_iter()
            .filter_map(|i| w.items.get(i).cloned())
            .collect()
    }

    /// What `id`'s picture shows, and the hash of what it is drawn from (the item files', and the
    /// skin's when the player is in it).
    fn shop_subject(&self, id: &str, c: &Catalogue, me: &Me) -> Option<(Subject, String)> {
        let items = self.shop_items(id, c, me);
        let hashes: Vec<&str> = (unpack(id, c, me).into_iter())
            .filter_map(|i| c.items.iter().find(|e| e.id == i))
            .map(|e| e.hash.as_str())
            .collect();
        let skin = (self.wardrobe.as_ref().and_then(|w| w.skin.as_ref()))
            .map_or(0, |(s, slim)| Arc::as_ptr(s) as usize + usize::from(*slim));
        let hash = format!("{}|{skin:x}", hashes.join(","));
        if let Some(set) = c.sets.iter().find(|s| s.id == id) {
            // The pack: its looks side by side, else its items worn together.
            let looks: Vec<Vec<Arc<Item>>> = (c.sets.iter())
                .filter(|s| s.id != set.id && s.items.iter().all(|i| set.items.contains(i)))
                .map(|s| self.shop_items(&s.id, c, me))
                .collect();
            if looks.len() > 1 {
                return Some((Subject::Pack(looks), hash));
            }
            return Some((Subject::Look(items), hash));
        }
        if id == FOUNDER_SET {
            return Some((Subject::Look(items), hash));
        }
        let item = items.into_iter().next()?;
        (item.slot != Slot::NameTag).then_some((Subject::Item(item), hash))
    }

    /// `id`'s picture `size` points large, drawn on a background thread the first time.
    fn shop_picture(
        &mut self,
        id: &str,
        size: (f32, f32),
        c: &Catalogue,
        me: &Me,
    ) -> Option<Arc<RenderImage>> {
        let (subject, hash) = self.shop_subject(id, c, me)?;
        let key = format!("{id}@{}x{}", size.0, size.1);
        let ask = Ask {
            key,
            hash,
            subject,
            size,
        };
        self.shop.thumbs.get(ask, self.scale)
    }

    /// Draws the pictures this frame asked for, each on a background thread, and shows each as
    /// it comes.
    fn shop_draw_pictures(&mut self, cx: &mut Context<Self>) {
        let asks = self.shop.thumbs.take_asks();
        if asks.is_empty() {
            return;
        }
        let (skin, slim) = (self.wardrobe.as_ref().and_then(|w| w.skin.clone()))
            .unwrap_or_else(|| (plain_skin(), false));
        let scale = self.scale;
        for ask in asks {
            let skin = skin.clone();
            let task = cx.background_executor().spawn(async move {
                let image = thumbs::bake(&ask.subject, &skin, slim, ask.size, scale);
                (ask.key, ask.hash, image)
            });
            cx.spawn(async move |this, cx| {
                let (key, hash, image) = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.shop.thumbs.put(key, hash, scale, image);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// What each slot shows now: what the account wears, then the open offer's items.
    fn shop_tried(&self, me: &Me, c: &Catalogue) -> BTreeMap<String, String> {
        let items: Vec<&str> = (self.shop.selected.iter())
            .flat_map(|id| unpack(id, c, me))
            .collect();
        let tried = items.into_iter().filter_map(|id| {
            let e = c.items.iter().find(|e| e.id == id)?;
            Some((e.slot.as_str(), e.id.as_str()))
        });
        rules::try_on(&me.equipped, tried)
    }

    /// The player in 3D wearing what is tried on, handed to the view when that changed (after
    /// this frame, as the Wardrobe does: the page never reads the view while it renders). An
    /// emote tried on plays, again each time it ends.
    fn shop_dress(&mut self, cx: &mut Context<Self>) -> Option<gpui::Entity<Viewer>> {
        let w = self.wardrobe.as_ref()?;
        let (me, c) = (w.saved.me.as_ref()?, w.saved.catalogue.as_ref()?);
        let worn = self.shop_tried(me, c);
        let (skin, slim) = w.skin.clone().unwrap_or_else(|| (plain_skin(), false));
        let emote = worn.get("emote").cloned();
        let outfit = Outfit {
            skin,
            slim,
            items: (worn.values().filter_map(|id| w.items.get(id)))
                .filter(|i| !matches!(i.slot, Slot::NameTag | Slot::Emote))
                .cloned()
                .collect(),
        };
        let motion = (emote.as_ref().and_then(|id| w.items.get(id).cloned()))
            .filter(|_| crate::sky::motion() != crate::sky::Motion::Still)
            .and_then(Played::motion);
        let viewer = (self.shop.viewer)
            .get_or_insert_with(|| cx.new(|_| Viewer::sized(VIEW.0, VIEW.1)))
            .clone();
        let now = (outfit.clone(), emote);
        if self.shop.shown.as_ref() != Some(&now) {
            let played = self.shop.shown.as_ref().map(|(_, e)| e.clone()) != Some(now.1.clone());
            self.shop.shown = Some(now);
            let animate = crate::sky::motion() != crate::sky::Motion::Still;
            let to = viewer.clone();
            cx.defer(move |cx| {
                to.update(cx, |v, cx| {
                    v.dress(outfit, animate, cx);
                    if played {
                        v.play_again(motion, cx);
                    }
                })
            });
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

    /// What an offer costs whoever gets it, or "Owned".
    fn shop_price(
        &self,
        id: &str,
        c: &Catalogue,
        owned: &HashSet<String>,
        currency: &str,
    ) -> String {
        if id == FOUNDER_SET {
            return String::new();
        }
        if let Some(s) = c.supporter.as_ref().filter(|s| s.item == id) {
            if owned.contains(id) {
                return tr("Owned").into();
            }
            let min = s.min.get(currency).copied().unwrap_or(0);
            return format!("{}+", rules::format_price(min, currency));
        }
        if let Some(s) = c.sets.iter().find(|s| s.id == id) {
            return match rules::set_price(s, c, owned, currency) {
                (_, mine) if mine == s.items.len() => tr("Owned").into(),
                (amount, _) => rules::format_price(amount, currency),
            };
        }
        if owned.contains(id) {
            return tr("Owned").into();
        }
        (c.items.iter().find(|e| e.id == id))
            .and_then(|e| e.prices.get(currency))
            .map_or_else(String::new, |p| rules::format_price(*p, currency))
    }

    /// A name tag style as the game draws it: the name, the style's mark after it, its line
    /// under it, `size` times the game's.
    fn shop_name_tag(&mut self, id: &str, c: &Catalogue, me: &Me, size: f32) -> Div {
        let scale = self.scale;
        let item = self
            .wardrobe
            .as_ref()
            .and_then(|w| w.items.get(id).cloned());
        let hash = c
            .items
            .iter()
            .find(|e| e.id == id)
            .map_or("", |e| e.hash.as_str());
        let line = (item.as_ref().and_then(|i| i.tag))
            .map(|g| gpui::rgb(u32::from_be_bytes([0, g.line[0], g.line[1], g.line[2]])));
        let mark = item.map(|i| {
            let key = format!("{id}@shop{size}");
            (self.wardrobe_marks)
                .get_or_bake(&key, hash, scale, || {
                    let t = &i.texture;
                    let texels: Vec<[u8; 4]> = t.rgba.as_chunks::<4>().0.to_vec();
                    art::bake(
                        &texels,
                        t.width as usize,
                        t.height as usize,
                        size * 2.,
                        scale,
                    )
                })
                .clone()
        });
        div()
            .flex()
            .items_center()
            .gap(px(6. * size))
            .px(px(8. * size))
            .py(px(3. * size))
            .bg(gpui::rgba(0x0000_0066))
            .child(
                div()
                    .font_family(theme::MONO)
                    .text_size(px(13. * size))
                    .text_color(gpui::white())
                    .child(me.name.clone()),
            )
            .children(mark.map(|m| gpui::img(m).size(px(16. * size))))
            .when_some(line, |d, c| d.border_b_2().border_color(c))
    }

    /// A tile: the thing drawn on its colour, its name, its price; lifted under the pointer.
    #[allow(clippy::too_many_arguments)]
    fn shop_tile(
        &mut self,
        id: &str,
        (w, h): (f32, f32),
        picture_h: f32,
        slot: &str,
        c: &Catalogue,
        me: &Me,
        price: String,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let lang = game_language();
        let (name, _) = about(id, c, lang);
        let up = self.shop.hover.as_deref() == Some(id);
        let colour: Hsla = ui::color(tint(slot));
        let picture: Div = if slot == "name_tag" {
            div().child(self.shop_name_tag(id, c, me, 1.6))
        } else {
            let image = self.shop_picture(id, (w, picture_h), c, me);
            div().children(image.map(|i| gpui::img(i).w(px(w)).h(px(picture_h))))
        };
        let (hovered, chosen) = (id.to_owned(), id.to_owned());
        div().w(px(w)).h(px(h)).child(
            keyboard(div().id(SharedString::from(format!("shop-tile-{id}"))), &t)
                .relative()
                .when(up, |d| d.top(px(-6.)))
                .flex()
                .flex_col()
                .w(px(w))
                .h(px(h))
                .overflow_hidden()
                .rounded(px(radius::PANEL))
                .cursor_pointer()
                .children(ground(colour, if up { 0.5 } else { 0.36 }))
                .border_1()
                .border_color(if up {
                    colour.opacity(0.9)
                } else {
                    ui::white(0.10)
                })
                .when(up, |d| d.shadow_lg())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(w))
                        .h(px(picture_h))
                        .flex_none()
                        .child(picture),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .px(px(14.))
                        .pb(px(12.))
                        .child(
                            ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                                .truncate()
                                .child(name),
                        )
                        .child(ui::text(14., FontWeight::MEDIUM, ink::BODY).child(price)),
                )
                .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                    let next = on.then(|| hovered.clone());
                    let mine = this.shop.hover.as_deref() == Some(hovered.as_str());
                    if (*on || mine) && this.shop.hover != next {
                        this.shop.hover = next;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.shop.selected = Some(chosen.clone());
                    this.shop.hover = None;
                    this.shop.said = None;
                    cx.notify();
                })),
        )
    }

    /// A row of tiles under its heading (none for the last row).
    fn shop_row(title: Option<String>, tiles: Vec<Div>) -> Option<Div> {
        (!tiles.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(title.map(|s| {
                    div()
                        .font_family(horizon_theme::glass::HEADING_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(22.))
                        .text_color(ui::white(ink::FULL))
                        .child(s)
                }))
                .child(div().flex().flex_wrap().gap(px(GAP)).children(tiles))
        })
    }

    /// The shop's front: the pack across the top, then the looks, each collection's items, and
    /// the founder set and the Supporter badge.
    fn shop_front(
        &mut self,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let lang = game_language();
        let owned: HashSet<String> = me.owned.iter().cloned().collect();
        let hero_id = hero(c).map(|s| s.id.clone());
        let mut out = div().flex().flex_col().gap(px(32.));
        if let Some(id) = &hero_id {
            out = out.child(self.shop_hero(id, me, c, currency, &owned, cx));
        }
        // The looks: every other set on sale.
        let looks: Vec<String> = (c.sets.iter())
            .filter(|s| !s.prices.is_empty() && Some(&s.id) != hero_id.as_ref())
            .map(|s| s.id.clone())
            .collect();
        let tiles = (looks.iter())
            .map(|id| {
                let price = self.shop_price(id, c, &owned, currency);
                self.shop_tile(id, LOOK, LOOK_PICTURE, "set", c, me, price, cx)
            })
            .collect();
        out = out.children(Self::shop_row(Some(tr("Looks").into()), tiles));
        // Each collection's items, then those of none.
        let badge = c.supporter.as_ref().map(|s| s.item.as_str());
        let known: HashSet<&str> = c.collections.iter().map(|g| g.id.as_str()).collect();
        let mut groups: Vec<(Option<&str>, Option<String>)> = (c.collections.iter())
            .map(|g| {
                (
                    Some(g.id.as_str()),
                    words(&g.names, lang).map(str::to_owned),
                )
            })
            .collect();
        groups.push((None, None));
        for (group, name) in groups {
            let entries: Vec<(String, String)> = (c.items.iter())
                .filter(|e| !e.prices.is_empty() && Some(e.id.as_str()) != badge)
                .filter(|e| match (group, e.collection.as_deref()) {
                    (Some(g), Some(of)) => g == of,
                    (None, of) => of.is_none_or(|of| !known.contains(of)),
                    _ => false,
                })
                .map(|e| (e.id.clone(), e.slot.clone()))
                .collect();
            let tiles = (entries.iter())
                .map(|(id, slot)| {
                    let price = self.shop_price(id, c, &owned, currency);
                    self.shop_tile(id, TILE, TILE_PICTURE, slot, c, me, price, cx)
                })
                .collect();
            out = out.children(Self::shop_row(name, tiles));
        }
        // The founder set while it can be claimed (or is owned), and the Supporter badge.
        let mut last = Vec::new();
        if me.claim.open || me.claim.claimed == Some(true) {
            let under = if me.claim.claimed == Some(true) {
                tr("Owned")
            } else {
                tr("Free for founders")
            };
            last.push(self.shop_tile(
                FOUNDER_SET,
                LOOK,
                LOOK_PICTURE,
                "set",
                c,
                me,
                under.into(),
                cx,
            ));
        }
        if let Some(s) = &c.supporter {
            let price = self.shop_price(&s.item, c, &owned, currency);
            last.push(self.shop_tile(&s.item, TILE, TILE_PICTURE, "name_tag", c, me, price, cx));
        }
        let any_sold = hero_id.is_some() || c.items.iter().any(|e| !e.prices.is_empty());
        if !any_sold && last.is_empty() {
            return out.child(
                ui::text(14., FontWeight::NORMAL, ink::BODY)
                    .child(tr("Nothing is on sale right now.")),
            );
        }
        out.children(Self::shop_row(None, last))
    }

    /// The pack, across the page: its looks drawn large, its name, what it holds, its price and
    /// one button, which opens it.
    fn shop_hero(
        &mut self,
        id: &str,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        owned: &HashSet<String>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = self.theme;
        let (name, _) = about(id, c, game_language());
        let n = unpack(id, c, me).len().to_string();
        let price = self.shop_price(id, c, owned, currency);
        let picture = self.shop_picture(id, HERO, c, me);
        let accent = ui::color(horizon_theme::glass::ACCENT);
        let warm = ui::color(tint("head"));
        let open = id.to_owned();
        let opened = id.to_owned();
        let buy = button("shop-hero-buy", "Buy", Kind::Primary, &t).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.shop.selected = Some(open.clone());
                this.shop.said = None;
                cx.notify();
            },
        ));
        div()
            .id("shop-hero")
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(px(HERO.1))
            .overflow_hidden()
            .rounded(px(radius::MODAL))
            .cursor_pointer()
            .children(ground(accent, 0.0).into_iter().take(1))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(gpui::linear_gradient(
                        100.,
                        gpui::linear_color_stop(warm.opacity(0.38), 0.),
                        gpui::linear_color_stop(accent.opacity(0.34), 1.),
                    )),
            )
            .border_1()
            .border_color(ui::white(0.12))
            .child(
                div()
                    .flex_none()
                    .w(px(HERO.0))
                    .h(px(HERO.1))
                    .children(picture.map(|i| gpui::img(i).w(px(HERO.0)).h(px(HERO.1)))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .flex_1()
                    .min_w_0()
                    .pr(px(36.))
                    .child(
                        div()
                            .font_family(horizon_theme::glass::DISPLAY_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(40.))
                            .line_height(px(44.))
                            .text_color(ui::white(ink::FULL))
                            .child(name),
                    )
                    .child(
                        ui::text(15., FontWeight::MEDIUM, ink::BODY)
                            .child(trf("{n} items", &[("n", &n)])),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(18.))
                            .pt(px(10.))
                            .child(ui::text(26., FontWeight::SEMIBOLD, ink::FULL).child(price))
                            .child(buy),
                    ),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.shop.selected = Some(opened.clone());
                this.shop.said = None;
                cx.notify();
            }))
    }

    /// An offer open big: the player wearing it in 3D, its name, line and price, Buy and Gift,
    /// and for a set, its items.
    fn shop_open(
        &mut self,
        id: &str,
        me: &Me,
        c: &Catalogue,
        currency: &str,
        viewer: Option<gpui::Entity<Viewer>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let lang = game_language();
        let (name, line) = about(id, c, lang);
        let back = button("shop-back", "←", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop.selected = None;
                this.shop.said = None;
                cx.notify();
            },
        ));
        // The player, with the name tag tried on over the head.
        let tag = self.shop_tried(me, c).remove("name_tag");
        let name_tag = match tag {
            Some(tag) => self.shop_name_tag(&tag, c, me, 1.),
            None => self.shop_name_tag("", c, me, 1.),
        };
        let reset = button("shop-reset", "Reset view", Kind::Ghost, &t).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                if let Some(v) = this.shop.viewer.clone() {
                    v.update(cx, |v, cx| v.reset(cx));
                }
            },
        ));
        let slot = (c.items.iter().find(|e| e.id == id)).map_or("set", |e| e.slot.as_str());
        let colour = ui::color(tint(slot));
        let player = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .flex_none()
            .p(px(20.))
            .relative()
            .overflow_hidden()
            .rounded(px(radius::MODAL))
            .children(ground(colour, 0.36))
            .border_1()
            .border_color(ui::white(0.10))
            .child(name_tag)
            .child(div().w(px(VIEW.0)).h(px(VIEW.1)).children(
                viewer.map(|v| viewer::frame(v, VIEW, self.shop.dragging, Self::shop_drag(cx))),
            ))
            .child(reset);
        let owned: HashSet<String> = me.owned.iter().cloned().collect();
        let price = self.shop_price(id, c, &owned, currency);
        let mut about_it = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(38.))
                    .line_height(px(42.))
                    .text_color(ui::white(ink::FULL))
                    .child(name),
            )
            .children(
                (!price.is_empty() && c.supporter.as_ref().is_none_or(|s| s.item != id))
                    .then(|| ui::text(24., FontWeight::SEMIBOLD, ink::FULL).child(price)),
            )
            .children(line.map(|l| {
                ui::text(14., FontWeight::NORMAL, ink::BODY)
                    .max_w(px(460.))
                    .line_height(px(21.))
                    .child(l)
            }));
        about_it = match id {
            FOUNDER_SET => about_it.children(self.shop_founder(me, cx)),
            _ => about_it.child(self.shop_buy(id, me, c, currency, cx)),
        };
        // A look's or the pack's items.
        let pieces = unpack(id, c, me);
        if pieces.len() > 1 {
            let mut grid = div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .pt(px(8.))
                .max_w(px(560.));
            for piece in pieces.iter().map(|p| p.to_string()) {
                let slot =
                    (c.items.iter().find(|e| e.id == piece)).map_or("set", |e| e.slot.as_str());
                let tile = self.shop_piece(&piece, slot, c, me);
                grid = grid.child(tile);
            }
            about_it = about_it.child(grid);
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().child(back))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(32.))
                    .child(player)
                    .child(about_it),
            )
    }

    /// One of a set's items in its big view: its picture and name.
    fn shop_piece(&mut self, id: &str, slot: &str, c: &Catalogue, me: &Me) -> Div {
        let (name, _) = about(id, c, game_language());
        let colour = ui::color(tint(slot));
        let picture: Div = if slot == "name_tag" {
            div().child(self.shop_name_tag(id, c, me, 0.8))
        } else {
            let image = self.shop_picture(id, (PIECE.0, PIECE_PICTURE), c, me);
            div().children(image.map(|i| gpui::img(i).w(px(PIECE.0)).h(px(PIECE_PICTURE))))
        };
        div()
            .flex()
            .flex_col()
            .w(px(PIECE.0))
            .h(px(PIECE.1))
            .overflow_hidden()
            .relative()
            .rounded(px(radius::LARGE))
            .children(ground(colour, 0.3))
            .border_1()
            .border_color(ui::white(0.08))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(PIECE.0))
                    .h(px(PIECE_PICTURE))
                    .flex_none()
                    .child(picture),
            )
            .child(
                ui::text(11., FontWeight::MEDIUM, ink::BODY)
                    .px(px(8.))
                    .truncate()
                    .child(name),
            )
    }

    /// The founder set: owned, or claimed here with one click where Paddle doesn't sell, else
    /// through Paddle's free checkout in the browser.
    fn shop_founder(&self, me: &Me, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        if me.claim.claimed == Some(true) {
            return Some(ui::text(15., FontWeight::MEDIUM, ink::FULL).child(tr("Owned")));
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
        let said = self
            .shop
            .said
            .clone()
            .map(|s| ui::text(13., FontWeight::NORMAL, ink::NOTE).child(s));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().flex().child(claim))
                .children(said),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_launcher::wardrobe::Collection;

    fn set(id: &str, items: &[&str], priced: bool) -> Set {
        Set {
            id: id.into(),
            items: items.iter().map(|s| s.to_string()).collect(),
            prices: if priced {
                [("USD".to_owned(), 999)].into()
            } else {
                Default::default()
            },
            ..Default::default()
        }
    }

    fn catalogue(sets: Vec<Set>, pack: Option<&str>) -> Catalogue {
        let mut c: Catalogue = serde_json::from_str(
            r#"{"items": [], "claim": {"state": "closed", "items": [], "open": false}}"#,
        )
        .unwrap();
        c.sets = sets;
        c.collections = vec![Collection {
            id: "autumn".into(),
            pack: pack.map(str::to_owned),
            ..Default::default()
        }];
        c
    }

    #[test]
    fn the_hero_is_the_collections_pack_else_the_largest_set_on_sale() {
        let look = set("look", &["a", "b"], true);
        let pack = set("pack", &["a", "b", "c"], true);
        let c = catalogue(vec![look.clone(), pack.clone()], Some("pack"));
        assert_eq!(hero(&c).map(|s| s.id.as_str()), Some("pack"));
        // The pack named isn't sold: the largest set that is.
        let c = catalogue(
            vec![look.clone(), set("pack", &["a", "b", "c"], false)],
            Some("pack"),
        );
        assert_eq!(hero(&c).map(|s| s.id.as_str()), Some("look"));
        let c = catalogue(vec![look, pack], None);
        assert_eq!(hero(&c).map(|s| s.id.as_str()), Some("pack"));
        assert!(hero(&catalogue(Vec::new(), Some("pack"))).is_none());
    }
}
