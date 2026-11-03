//! The Wardrobe page's Shop tab: the founder set, free to claim while its window is open, then
//! retired (`horizon_launcher::shop` holds the rules). It shows only once the cosmetics service
//! has a window, or previews it for the account; nothing in it asks for money.
//!
//! Signed in, it reads the Wardrobe's own trip to the service (the kept outfit first, then the
//! answer); signed out, it asks for the catalogue alone, to know whether there is a Shop to show.
use super::*;
use horizon_launcher::shop::{self as rules, Window};
use horizon_launcher::wardrobe::{Entry, Me};
use horizon_theme::glass::{ink, radius};

/// The founder panel's left column.
const LEFT: f32 = 300.;
/// A card's picture.
const THUMB: f32 = 120.;
/// How often the time left is drawn again.
const TICK: Duration = Duration::from_secs(30);

/// The Shop tab's own state; the account's items are the Wardrobe's.
#[derive(Default)]
pub(crate) struct Shop {
    /// The Shop tab is the one shown (else the Wardrobe's).
    pub(super) open: bool,
    /// A claim on its way.
    claiming: bool,
    /// The claim went through on this visit: the panel says "Claimed".
    claimed_now: bool,
    /// The last claim's failure, in words, and the line to copy.
    failed: Option<(String, String)>,
    /// Signed out: the catalogue, asked without an account.
    anon: Option<Catalogue>,
    asking: bool,
    /// Asked on this visit (a failure waits for the next one).
    asked: bool,
    /// A redraw for the time left is due.
    ticking: bool,
    /// Cards' pictures, baked at a window scale.
    thumbs: HashMap<String, (f32, Arc<RenderImage>)>,
}

/// Text at `size` in Geist, in the page's ink at `a`.
fn words(t: &Theme, size: f32, weight: FontWeight, a: f32) -> Div {
    glass_ui::text(size, weight, a).text_color(tone(t, a))
}

/// The page's ink at `a`: white on the glass look, the theme's ink on the light one.
fn tone(t: &Theme, a: f32) -> Hsla {
    if t.dark {
        glass_ui::white(a)
    } else {
        t.ink.opacity(a)
    }
}

/// A small label (`chip`): 24 px, round at 8, 12 px medium.
#[derive(Clone, Copy)]
enum Chip {
    /// The accent at 30 %: Free, Equipped.
    Accent,
    /// Green: Owned, Yours.
    Live,
    /// An outline only: dates.
    Ghost,
}

fn chip(kind: Chip, label: impl Into<SharedString>, t: &Theme) -> Div {
    let look = glass_ui::look();
    let d = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .px(px(9.))
        .rounded(px(8.))
        .font_family(horizon_theme::glass::FONT)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap();
    let d = match kind {
        Chip::Accent => d
            .bg(glass_ui::color(look.accent).opacity(0.3))
            .text_color(tone(t, ink::FULL)),
        Chip::Live => d
            .bg(glass_ui::color(horizon_theme::glass::LIVE).opacity(0.16))
            .text_color(if t.dark {
                glass_ui::color(0xb8f5cf)
            } else {
                t.success
            }),
        Chip::Ghost => d
            .border_1()
            .border_color(tone(t, 0.16))
            .text_color(tone(t, ink::BODY)),
    };
    d.child(label.into())
}

/// A dot before a line: the accent's soft shade, green, amber or the danger colour.
fn dot(c: u32) -> Div {
    div()
        .flex_none()
        .size(px(7.))
        .rounded_full()
        .bg(glass_ui::color(c))
}

/// A card's dark plate (`.item`): round at 16, 1 px violet edge; a 2 px ring in the accent's soft
/// shade when the item is worn.
fn card_plate(t: &Theme, worn: bool) -> Div {
    let look = glass_ui::look();
    let d = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .p(px(8.))
        .pb(px(12.))
        .rounded(px(16.));
    if !t.dark {
        return d
            .bg(t.well)
            .border_2()
            .border_color(if worn { t.accent } else { t.outline });
    }
    d.bg(gpui::rgba(0x15112ed6))
        .when(worn, |d| {
            d.border_2().border_color(glass_ui::color(look.accent_soft))
        })
        .when(!worn, |d| d.border_1().border_color(gpui::rgba(0xb9acff1f)))
}

/// The picture's dark slot in a card.
fn slot_well(t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(THUMB))
        .w_full()
        .overflow_hidden()
        .rounded(px(radius::ROW))
        .bg(if t.dark {
            gpui::rgba(0x080614b3).into()
        } else {
            t.plate
        })
}

/// The empty states' plate (`states.html`): a title line, then centred lines and actions.
fn state_card(t: &Theme) -> Div {
    plate(t)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.))
        .min_h(px(300.))
        .p(px(28.))
}

/// A state's headline (16 px medium) and its explanation (14 px at 60 %, at most 46 characters).
fn state_text(t: &Theme, title: String, note: String) -> [Div; 2] {
    [
        words(t, 16., FontWeight::MEDIUM, ink::FULL).child(title),
        words(t, 14., FontWeight::NORMAL, ink::NOTE)
            .max_w(px(420.))
            .text_center()
            .child(note),
    ]
}

/// A slot's name, as the Wardrobe's rows say it.
fn slot_name(slot: &str) -> &'static str {
    match slot {
        "head" => tr("Head"),
        "back" => tr("Back"),
        "shoulder" => tr("Shoulder pet"),
        "name_tag" => tr("Name tag"),
        "theme" => tr("Launcher theme"),
        _ => "",
    }
}

/// The drawn part of `rgba` (`w` × `h`), centred in a `side` square with a margin, each texel
/// the mean of the ones it covers. `None` when nothing was drawn.
fn fit(rgba: &[u8], w: usize, h: usize, side: usize) -> Option<Vec<[u8; 4]>> {
    let drawn = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3] > 0;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if drawn(x, y) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x1 <= x0 {
        return None;
    }
    // A fifth of the well stays empty around the item.
    let span = ((x1 - x0).max(y1 - y0) as f32 * 1.25).ceil();
    let (cx, cy) = ((x0 + x1) as f32 / 2., (y0 + y1) as f32 / 2.);
    let step = span / side as f32;
    let mut out = vec![[0u8; 4]; side * side];
    for (i, px) in out.iter_mut().enumerate() {
        let (ox, oy) = ((i % side) as f32, (i / side) as f32);
        let (sx, sy) = (cx - span / 2. + ox * step, cy - span / 2. + oy * step);
        let (mut sum, mut n) = ([0f32; 4], 0f32);
        for y in sy.floor() as i64..(sy + step).ceil() as i64 {
            for x in sx.floor() as i64..(sx + step).ceil() as i64 {
                n += 1.;
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let t = &rgba[(y as usize * w + x as usize) * 4..][..4];
                let a = t[3] as f32 / 255.;
                for c in 0..3 {
                    sum[c] += t[c] as f32 * a;
                }
                sum[3] += a;
            }
        }
        if sum[3] > 0. {
            let rgb = |c: usize| (sum[c] / sum[3]).round() as u8;
            *px = [rgb(0), rgb(1), rgb(2), (sum[3] / n * 255.).round() as u8];
        }
    }
    Some(out)
}

/// `e`'s failure as the line to copy: host · what · when.
fn details(e: &Error) -> String {
    let code = match e {
        Error::Service(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    let at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    format!("{} · {code} · {at}", Service::live().host())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl Launcher {
    /// Whether the Shop tab shows: the service's public answer, or the account's own (a preview,
    /// a claim), gives it something to show (`shop::shown`), whatever the cosmetics switch says.
    fn shop_shown(&self) -> bool {
        let own = (self.wardrobe.as_ref())
            .and_then(|w| w.saved.me.as_ref())
            .map(|m| &m.claim)
            .or(self.shop_kept.as_ref());
        let anon = self.shop.anon.as_ref().map(|c| &c.claim);
        [self.shop_status.as_ref(), anon, own]
            .into_iter()
            .flatten()
            .any(rules::shown)
    }

    /// Each draw of the page: a new visit forgets the last claim's outcome; signed out, the
    /// catalogue is asked once a visit.
    pub(super) fn shop_visit(&mut self, new_visit: bool, signed_out: bool, cx: &mut Context<Self>) {
        if new_visit {
            self.shop.claimed_now = false;
            self.shop.failed = None;
            self.shop.asked = false;
        }
        if signed_out && !self.shop.asked {
            self.shop_ask(cx);
        }
    }

    /// Signed out: ask for the catalogue alone (no account, nothing kept), to learn whether the
    /// Shop shows.
    pub(super) fn shop_ask(&mut self, cx: &mut Context<Self>) {
        if self.shop.asking {
            return;
        }
        self.shop.asking = true;
        self.shop.asked = true;
        let task = cx
            .background_executor()
            .spawn(async { Service::live().catalogue() });
        cx.spawn(async move |this, cx| {
            let got = task.await;
            let _ = this.update(cx, |this, cx| {
                this.shop.asking = false;
                if let Ok(c) = got {
                    this.shop.anon = Some(c);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The Wardrobe | Shop tabs over the page, or None while there is no Shop (then the
    /// Wardrobe is all the page shows). Offline, the Shop's tab is dimmed, not hidden.
    pub(super) fn shop_tabs(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.shop_shown() {
            self.shop.open = false;
            return None;
        }
        let t = self.theme;
        let offline = self
            .wardrobe
            .as_ref()
            .is_some_and(|w| w.status == Status::Offline);
        let owned = self
            .wardrobe
            .as_ref()
            .and_then(|w| w.saved.me.as_ref())
            .map(|m| m.owned.len());
        let open = self.shop.open;
        let tab = |id: &'static str, label: &str, count: Option<usize>, on: bool| {
            glass_ui::tab(id, tr(label), on, &t)
                .h(px(32.))
                .px(px(12.))
                .text_size(px(13.))
                .when(!t.dark, |d| d.text_color(t.ink).when(on, |d| d.bg(t.well)))
                .children(count.map(|n| div().text_color(tone(&t, ink::DIM)).child(n.to_string())))
        };
        let wardrobe = tab("wardrobe-tab", "Wardrobe", owned, !open).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.shop.open = false;
                cx.notify();
            },
        ));
        let shop = tab("shop-tab", "Shop", None, open)
            .when(offline && !open, |d| d.opacity(0.4))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.shop.open = true;
                cx.notify();
            }));
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .pt(px(28.))
                .child(
                    plate(&t)
                        .flex()
                        .gap(px(4.))
                        .p(px(3.))
                        .child(wardrobe)
                        .child(shop),
                )
                .child(
                    words(&t, 13., FontWeight::MEDIUM, ink::NOTE)
                        .ml_auto()
                        .text_right()
                        .child(tr(
                            "Cosmetics show only to Horizon players. Nothing changes how the game plays.",
                        )),
                ),
        )
    }

    /// The Shop tab under `tabs`.
    pub(super) fn shop_page(&mut self, tabs: Div, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        self.shop_bake();
        let head = page_header(
            "Shop",
            "Things to wear for your player, seen by everyone who plays on Horizon.",
            &t,
        )
        .pt(px(28.));
        let body = self.shop_body(cx);
        let toast = self
            .wardrobe
            .as_ref()
            .and_then(|w| w.toast.as_ref())
            .filter(|(_, at)| at.elapsed() < TOAST)
            .map(|(s, _)| s.clone());
        div()
            .flex()
            .flex_col()
            .child(tabs)
            .child(head)
            .child(body)
            .children(toast.map(|s| {
                div().flex().justify_center().mt(px(16.)).child(
                    crate::glass::float(radius::POPOVER)
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(16.))
                        .py(px(12.))
                        .child(dot(horizon_theme::glass::LIVE).relative())
                        .child(
                            words(&t, 13., FontWeight::NORMAL, ink::FULL)
                                .relative()
                                .child(s),
                        ),
                )
            }))
    }

    /// What the tab shows for where the account and the service stand.
    fn shop_body(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let Some(w) = self.wardrobe.as_ref() else {
            // Signed out: the set, with Microsoft's button where the claim goes.
            let Some(c) = self.shop.anon.clone() else {
                return self.shop_loading();
            };
            return self.founder_panel(&c, None, cx);
        };
        match w.status.clone() {
            Status::Offline => {
                let waiting = w.saved.pending.len();
                let [a, b] = state_text(
                    &t,
                    tr("You're offline.").into(),
                    tr(
                        "The shop needs a connection. You can still change what you wear in the \
                        Wardrobe: it saves on this computer and syncs when you're back online.",
                    )
                    .into(),
                );
                return state_card(&t).child(a).child(b).when(waiting > 0, |d| {
                    d.child(
                        chip(
                            Chip::Ghost,
                            trf(
                                "Changes waiting to sync: {n}",
                                &[("n", &waiting.to_string())],
                            ),
                            &t,
                        )
                        .child(dot(horizon_theme::glass::WARN)),
                    )
                });
            }
            Status::Failed(line) => {
                let [a, b] = state_text(
                    &t,
                    tr("The shop didn't load.").into(),
                    tr(
                        "What you wear still shows in the game: the launcher kept your last \
                        outfit. Try again in a moment.",
                    )
                    .into(),
                );
                let copy = line.clone();
                return state_card(&t)
                    .child(dot(horizon_theme::glass::DANGER).size(px(9.)))
                    .child(a)
                    .child(b)
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(self.wardrobe_retry(cx))
                            .child(
                                button("shop-details", "Copy details", Kind::Ghost, &t).on_click(
                                    move |_: &ClickEvent, _, cx| {
                                        copy_to_clipboard(
                                            cx,
                                            gpui::ClipboardItem::new_string(copy.clone()),
                                        )
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(11.))
                            .text_color(tone(&t, ink::PLACEHOLDER))
                            .child(line),
                    );
            }
            Status::Account(said) => {
                let [a, b] = state_text(&t, tr("The shop didn't load.").into(), said);
                return state_card(&t)
                    .child(dot(horizon_theme::glass::DANGER).size(px(9.)))
                    .child(a)
                    .child(b)
                    .child(self.wardrobe_retry(cx));
            }
            Status::Loading | Status::Ready => {}
        }
        let (Some(c), Some(me)) = (w.saved.catalogue.clone(), w.saved.me.clone()) else {
            return self.shop_loading();
        };
        self.founder_panel(&c, Some(me), cx)
    }

    /// Before the first answer: placeholders at the panel's size, no spinner.
    fn shop_loading(&self) -> Div {
        let t = self.theme;
        let bar =
            |w: f32, h: f32, a: f32| div().w(px(w)).h(px(h)).rounded(px(h / 2.)).bg(tone(&t, a));
        let card = || {
            card_plate(&t, false)
                .child(slot_well(&t).bg(tone(&t, 0.06)))
                .child(bar(84., 12., 0.1).mt(px(12.)).mx(px(4.)))
                .child(bar(56., 10., 0.07).mt(px(8.)).mx(px(4.)))
        };
        crate::glass::plate(radius::PANEL)
            .flex()
            .gap(px(24.))
            .p(px(24.))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .w(px(LEFT))
                    .flex_none()
                    .child(bar(52., 24., 0.1))
                    .child(bar(220., 30., 0.1))
                    .child(bar(260., 12., 0.07))
                    .child(bar(200., 12., 0.07)),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .gap(px(12.))
                    .children((0..4).map(|_| card())),
            )
    }

    /// The founder set: what it is, until when, the one action, and its items. `me` is None
    /// signed out.
    fn founder_panel(&mut self, c: &Catalogue, me: Option<Me>, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let lang = game_language();
        let claim = me.as_ref().map_or(&c.claim, |m| &m.claim);
        let window = rules::window(claim, now());
        let owned = me.as_ref().is_some_and(rules::owns_set);
        // In the catalogue's order, so the cards don't move when the account's answer changes.
        let set = if c.claim.items.is_empty() {
            &claim.items
        } else {
            &c.claim.items
        };
        let entries: Vec<Entry> = set
            .iter()
            .filter_map(|id| c.items.iter().find(|e| &e.id == id).cloned())
            .collect();
        // Over, and never claimed: the items are gone.
        if let (Window::Closed { ended_at }, false) = (window, owned) {
            let title = match ended_at {
                Some(end) => trf(
                    "The founder set was free to claim until {date}.",
                    &[("date", &rules::until(end))],
                ),
                None => tr("The founder set isn't offered any more.").into(),
            };
            let [a, b] = state_text(
                &t,
                title,
                tr("It isn't offered any more, and it won't be sold. Players who claimed it keep it.")
                    .into(),
            );
            return state_card(&t).child(a).child(b).child(
                button("shop-back", "Back to the wardrobe", Kind::Ghost, &t).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.shop.open = false;
                        cx.notify();
                    },
                )),
            );
        }
        let look = glass_ui::look();
        // The chip over the title, the paragraph, and the line with the time.
        let (badge, said, clock) = match (window, owned) {
            (_, true) => (
                chip(Chip::Live, tr("Yours"), &t),
                match window {
                    Window::Closed {
                        ended_at: Some(end),
                    } => trf(
                        "It was free to claim until {date}. It isn't offered any more and won't \
                         be sold; yours stays yours.",
                        &[("date", &rules::until(end))],
                    ),
                    _ => tr("It's in your wardrobe, yours to keep. Wear it from the Wardrobe tab.")
                        .into(),
                },
                None,
            ),
            (Window::Open { ends_at: None }, false) => (
                chip(Chip::Accent, tr("Free for founders"), &t),
                tr(
                    "For the players who were here first. Free to claim; once claimed, it's \
                    yours to keep.",
                )
                .into(),
                None,
            ),
            (
                Window::Open {
                    ends_at: Some(ends_at),
                },
                false,
            ) => (
                chip(
                    Chip::Accent,
                    trf(
                        "Free for founders, until {date}",
                        &[("date", &rules::until(ends_at))],
                    ),
                    &t,
                ),
                trf(
                    "For the players who were here first. Free to claim until {date}; once \
                     claimed, it's yours to keep.",
                    &[("date", &rules::until(ends_at))],
                ),
                Some(trf(
                    "{time} left to claim it",
                    &[("time", &rules::span(now(), ends_at))],
                )),
            ),
            (
                Window::NotYet {
                    starts_at: Some(start),
                },
                false,
            ) => (
                chip(
                    Chip::Ghost,
                    trf("Opens {date}", &[("date", &rules::from(start))]),
                    &t,
                ),
                trf(
                    "For the players who were here first. Free to claim from {date}; once \
                     claimed, it's yours to keep.",
                    &[("date", &rules::from(start))],
                ),
                Some(trf(
                    "Opens in {time}",
                    &[("time", &rules::span(now(), start))],
                )),
            ),
            (_, false) => (
                chip(Chip::Ghost, tr("Not open yet"), &t),
                tr("For the players who were here first. The dates to claim it aren't set yet.")
                    .into(),
                None,
            ),
        };
        if clock.is_some() {
            self.shop_tick(cx);
        }
        // Open for this account only, ahead of everyone: say so.
        let preview = claim.preview.then(|| chip(Chip::Ghost, tr("Preview"), &t));
        let action = self.founder_action(window, owned, me.is_some(), cx);
        let note = match (me.is_some(), owned) {
            (false, _) => tr(
                "Sign in to claim it. Cosmetics belong to the Microsoft account that owns \
                 Minecraft: Java Edition, so they follow you to every computer.",
            ),
            (true, true) => {
                tr("Bound to your Minecraft account, on every computer you sign in on.")
            }
            (true, false) => tr("Bound to your Minecraft account. Nothing to pay, no card asked."),
        };
        let failed = self.shop.failed.clone().map(|(said, line)| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mt(px(10.))
                .child(dot(horizon_theme::glass::DANGER))
                .child(
                    words(&t, 13., FontWeight::NORMAL, ink::BODY)
                        .flex_1()
                        .min_w_0()
                        .child(said),
                )
                .child(
                    glass_ui::link("shop-claim-details", tr("Copy details"))
                        .text_size(px(13.))
                        .text_color(tone(&t, ink::FULL))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            copy_to_clipboard(cx, gpui::ClipboardItem::new_string(line.clone()))
                        }),
                )
        });
        let left = div()
            .relative()
            .flex()
            .flex_col()
            .justify_between()
            .gap(px(16.))
            .w(px(LEFT))
            .flex_none()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().flex().gap(px(8.)).child(badge).children(preview))
                    .child(
                        div()
                            .mt(px(14.))
                            .font_family(horizon_theme::glass::DISPLAY_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(34.))
                            .line_height(px(36.))
                            .text_color(tone(&t, ink::FULL))
                            .child(tr("The founder set")),
                    )
                    .child(
                        words(&t, 14., FontWeight::NORMAL, ink::NOTE)
                            .mt(px(8.))
                            .line_height(px(21.7))
                            .child(said),
                    )
                    .children(clock.map(|c| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mt(px(12.))
                            .child(dot(look.accent_soft))
                            .child(words(&t, 13., FontWeight::MEDIUM, ink::SOFT).child(c))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(action)
                    .child(
                        words(&t, 13., FontWeight::NORMAL, ink::NOTE)
                            .mt(px(8.))
                            .child(note),
                    )
                    .children(failed),
            );
        let cards = entries
            .iter()
            .map(|e| self.founder_card(e, &window, me.as_ref(), lang))
            .collect::<Vec<_>>();
        crate::glass::plate(radius::PANEL)
            .flex()
            .gap(px(24.))
            .p(px(24.))
            .child(left)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(12.))
                    .children(cards),
            )
    }

    /// The panel's one action: Claim (the screen's accent), Claiming, Open the wardrobe once
    /// the set is the account's, Microsoft's button signed out.
    fn founder_action(
        &self,
        window: Window,
        owned: bool,
        signed_in: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.theme;
        if !signed_in {
            return microsoft::sign_in_button("shop-sign-in", &t)
                .justify_center()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.go(Page::Settings);
                    this.start_sign_in();
                    cx.notify();
                }))
                .into_any_element();
        }
        let wide = |b: Stateful<Div>| b.w_full().h(px(48.)).rounded(px(radius::LARGE));
        if owned {
            let label = if self.shop.claimed_now {
                "Claimed · Open the wardrobe"
            } else {
                "Open the wardrobe"
            };
            return wide(button("shop-wardrobe", label, Kind::Secondary, &t))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.shop.open = false;
                    cx.notify();
                }))
                .into_any_element();
        }
        let ready = self
            .wardrobe
            .as_ref()
            .is_some_and(|w| !w.busy && w.session.as_ref().is_some_and(|s| s.live()));
        let label = if self.shop.claiming {
            "Claiming…"
        } else {
            "Claim the founder set"
        };
        let b = wide(button("shop-claim", label, Kind::Primary, &t)).text_size(px(16.));
        if !matches!(window, Window::Open { .. }) || !ready || self.shop.claiming {
            return disabled(b).into_any_element();
        }
        b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.shop_claim(cx);
            cx.notify();
        }))
        .into_any_element()
    }

    /// One founder item: its picture, name, slot and where it stands for the account.
    fn founder_card(&self, e: &Entry, window: &Window, me: Option<&Me>, lang: &str) -> Div {
        let t = self.theme;
        let worn = me.is_some_and(|m| m.equipped.get(&e.slot) == Some(&e.id));
        let owned = me.is_some_and(|m| m.owned.contains(&e.id));
        let state = if worn {
            Some(chip(Chip::Accent, tr("Equipped"), &t))
        } else if owned {
            Some(chip(Chip::Live, tr("Owned"), &t))
        } else {
            match window {
                Window::Open { .. } => Some(chip(Chip::Accent, tr("Free"), &t)),
                Window::NotYet { .. } => Some(chip(Chip::Ghost, tr("Soon"), &t)),
                Window::Closed { .. } => None,
            }
        };
        let picture: AnyElement = if let Some(sky) = e.theme.as_ref().and_then(|th| th.sky()) {
            // A launcher theme: its sky's six bands.
            div()
                .flex()
                .flex_col()
                .size_full()
                .children(sky.map(|c| div().flex_1().w_full().bg(gpui::rgb(c))))
                .into_any_element()
        } else if e.slot == "name_tag" {
            self.tag_preview(e, me)
        } else if let Some((_, image)) = self.shop.thumbs.get(&e.id) {
            gpui::img(image.clone())
                .w(px(THUMB))
                .h(px(THUMB))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        card_plate(&t, worn)
            .child(slot_well(&t).child(picture))
            .child(
                words(&t, 14., FontWeight::SEMIBOLD, ink::FULL)
                    .mt(px(10.))
                    .mx(px(4.))
                    .line_height(px(18.))
                    .child(e.name(lang).to_owned()),
            )
            .child(
                // The slot, then the chip under it at the card's foot: a narrow card has no room
                // for both on one line.
                words(&t, 12., FontWeight::NORMAL, ink::QUIET)
                    .mt(px(2.))
                    .mx(px(4.))
                    .child(slot_name(&e.slot)),
            )
            .child(div().flex().mt_auto().pt(px(8.)).mx(px(4.)).children(state))
    }

    /// A name tag style on the account's name: its mark, the name, its line under them.
    fn tag_preview(&self, e: &Entry, me: Option<&Me>) -> AnyElement {
        let item = self
            .wardrobe
            .as_ref()
            .and_then(|w| w.items.get(&e.id))
            .filter(|i| i.tag.is_some())
            .cloned();
        let line = item
            .as_ref()
            .and_then(|i| i.tag)
            .map(|g| gpui::rgb(u32::from_be_bytes([0, g.line[0], g.line[1], g.line[2]])));
        let mark = item.as_ref().map(|i| self.wardrobe_mark(i));
        let name = me.map_or_else(|| tr("Your name").to_owned(), |m| m.name.clone());
        // Small enough for a 16-character name in a narrow card; a longer one ends in "…".
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .max_w(px(THUMB - 16.))
            .px(px(5.))
            .py(px(3.))
            .bg(gpui::rgba(0x0000_0066))
            .children(mark.map(|m| gpui::img(m).flex_none().size(px(12.))))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(theme::MONO)
                    .text_size(px(10.))
                    .text_color(gpui::white())
                    .child(name),
            )
            .when_some(line, |d, c| d.border_b_2().border_color(c))
            .into_any_element()
    }

    /// Bake the cards' pictures the Wardrobe has the items for: the item alone, three-quarter on
    /// (back items from behind), framed to fill the card's well.
    fn shop_bake(&mut self) {
        let scale = self.scale;
        let Some(w) = self.wardrobe.as_ref() else {
            return;
        };
        let body = preview::player_model(false);
        let side = (THUMB * scale) as u32;
        // Drawn where it sits on a whole body, four times finer than the card, then cut to it.
        let (wide, tall) = (side * 6, side * 10);
        let mut baked = Vec::new();
        for (id, item) in &w.items {
            if self.shop.thumbs.get(id).is_some_and(|(s, _)| *s == scale) {
                continue;
            }
            let Some(draw) = preview::Draw::item(item, &body, 0.0) else {
                continue;
            };
            let yaw = if item.slot == horizon_cosmetics::Slot::Back {
                210.
            } else {
                -30.
            };
            let image = preview::render(&[draw], wide, tall, yaw, 12.);
            let Some(texels) = fit(&image.rgba, wide as usize, tall as usize, side as usize) else {
                continue;
            };
            let pic = art::bake(&texels, side as usize, side as usize, 1. / scale, scale);
            baked.push((id.clone(), (scale, pic)));
        }
        self.shop.thumbs.extend(baked);
    }

    /// Draw the time left again in a while, while it shows.
    fn shop_tick(&mut self, cx: &mut Context<Self>) {
        if self.shop.ticking {
            return;
        }
        self.shop.ticking = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TICK).await;
            let _ = this.update(cx, |this, cx| {
                this.shop.ticking = false;
                if this.page == Page::Wardrobe && this.shop.open {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Claim the founder set from the Shop: the button says "Claiming…" until the service
    /// answers. The window closing meanwhile, or the session ending, asks the service again.
    fn shop_claim(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wardrobe.as_mut().filter(|w| !w.busy) else {
            return;
        };
        let Some(session) = w.session.clone().filter(|s| s.live()) else {
            return;
        };
        w.busy = true;
        self.shop.claiming = true;
        self.shop.failed = None;
        let (uuid, mut saved) = (w.uuid.clone(), w.saved.clone());
        let task = cx.background_executor().spawn(async move {
            let result = Service::live()
                .claim(&session)
                .map(|me| saved.me = Some(me));
            if result.is_ok() {
                let _ = saved.save(&dir(), &uuid);
            }
            (saved, result)
        });
        cx.spawn(async move |this, cx| {
            let (saved, result) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.shop.claiming = false;
                match result {
                    Ok(()) => {
                        this.shop.claimed_now = true;
                        let trip = Trip {
                            saved,
                            items: HashMap::new(),
                            session: None,
                            result: Ok(()),
                        };
                        let done = tr("Founder set claimed. It's in your wardrobe.").into();
                        this.wardrobe_back(trip, Some(done));
                    }
                    Err(e @ (Error::ClaimClosed | Error::SignedOut)) => {
                        // The window moved or the session ended: ask the service where it
                        // stands now.
                        this.open_wardrobe(cx);
                        if let Some(w) = this.wardrobe.as_mut() {
                            w.toast = Some((e.message(), Instant::now()));
                        }
                    }
                    Err(e) => {
                        if let Some(w) = this.wardrobe.as_mut() {
                            w.busy = false;
                        }
                        this.shop.failed = Some((
                            trf(
                                "The claim didn't go through: {why}",
                                &[("why", &e.message())],
                            ),
                            details(&e),
                        ));
                    }
                }
                cx.notify();
            });
            cx.background_executor().timer(TOAST).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
    /// The Wardrobe's screenshot preview (`wardrobe_preview`) on the Shop tab, in `state`:
    /// `open`, `claiming`, `claimed`, `owned`, `error` (the claim failed), `not-yet`, `preview`
    /// (open for this account only, no dates), `closed`, `closed-owned`, `loading`, `offline`,
    /// `failed`, `account`, `signed-out`. Development builds only; nothing reaches the service.
    pub(super) fn shop_preview(&mut self, state: &str) {
        use horizon_launcher::wardrobe::ClaimState;
        // On the hour, as the service's dates are.
        let at = now() / 3600 * 3600;
        const DAY: i64 = 86_400;
        self.shop = Shop {
            open: true,
            asked: true,
            ..Default::default()
        };
        let Some(w) = self.wardrobe.as_mut() else {
            return;
        };
        w.status = Status::Ready;
        let (Some(c), Some(me)) = (w.saved.catalogue.as_mut(), w.saved.me.as_mut()) else {
            return;
        };
        let all = c.claim.items.clone();
        me.name = "Player46008".into();
        let (claim_state, starts, ends) = match state {
            "not-yet" => (
                ClaimState::NotYet,
                Some(at + 3 * DAY + 4 * 3600),
                Some(at + 90 * DAY),
            ),
            "preview" => (ClaimState::Open, None, None),
            "closed" | "closed-owned" => (ClaimState::Closed, Some(at - 120 * DAY), Some(at - DAY)),
            _ => (
                ClaimState::Open,
                Some(at - 2 * DAY),
                Some(at + 12 * DAY + 5 * 3600),
            ),
        };
        let owns = matches!(state, "claimed" | "owned" | "closed-owned");
        for claim in [&mut c.claim, &mut me.claim] {
            claim.state = claim_state;
            claim.starts_at = starts;
            claim.ends_at = ends;
        }
        me.claim.claimed = Some(owns);
        for claim in [&mut c.claim, &mut me.claim] {
            claim.open = matches!(claim_state, ClaimState::Open);
            claim.preview = state == "preview";
        }
        // The public answer keeps no window in a preview.
        if state == "preview" {
            c.claim.open = false;
        }
        // What `GET /v1/shop` would say: the page shows while the account's answer loads.
        self.shop_status = Some(c.claim.clone());
        me.owned = if owns { all } else { Vec::new() };
        me.equipped.clear();
        if state == "owned" {
            me.equipped.insert("head".into(), "founder_visor".into());
        }
        match state {
            "claiming" => self.shop.claiming = true,
            "claimed" => {
                self.shop.claimed_now = true;
                w.toast = Some((
                    tr("Founder set claimed. It's in your wardrobe.").into(),
                    Instant::now() + Duration::from_secs(3600),
                ));
            }
            "error" => {
                self.shop.failed = Some((
                    trf(
                        "The claim didn't go through: {why}",
                        &[("why", &Error::Service(503).message())],
                    ),
                    "cosmetics.playhorizon.dev · 503 · 2026-11-03 20:41:07".into(),
                ))
            }
            "loading" => {
                w.status = Status::Loading;
                w.saved.me = None;
            }
            "offline" => {
                w.status = Status::Offline;
                w.saved.change(Change::Equip {
                    slot: "head".into(),
                    item: None,
                });
            }
            "failed" => {
                w.status =
                    Status::Failed("cosmetics.playhorizon.dev · 503 · 2026-11-03 20:41".into())
            }
            "account" => {
                w.status = Status::Account(
                    tr("Mojang could not confirm your account. Sign in again and retry.").into(),
                )
            }
            "signed-out" => {
                let uuid = w.uuid.clone();
                self.shop.anon = w.saved.catalogue.clone();
                self.wardrobe = None;
                self.accounts.accounts.retain(|a| a.uuid != uuid);
            }
            _ => {}
        }
    }
}
