//! The Shop's rules, beside its page (`wardrobe_page/shop.rs`):
//!
//! - Whether the top bar has the Wardrobe and Shop pages at all: only when Horizon's cosmetics
//!   service says there is something to show, whatever the cosmetics switch says. The service
//!   decides with its founder-set window (`GET /v1/shop`): the set can be claimed now (the public
//!   window, or a preview for an account on its list), the account claimed it, or the public
//!   window has dates. With no window set, the launcher shows neither.
//! - A basket's prices, before the checkout, with the service's own rules (the website's shop
//!   does the same, `components/shop/pricing.ts` in horizon-site): an item the account owns drops
//!   out, an item also inside a set in the same basket drops out, a set partly owned costs its
//!   price times the share of its items not owned yet. The checkout's total is what Paddle
//!   charges; the launcher only previews it and never decides a price. Every price comes from
//!   the service's catalogue: what it doesn't price isn't sold.
//! - Paying happens on the website: the service starts Paddle's checkout and the launcher opens
//!   [`pay_url`] in the browser. No card is ever typed into the launcher.
use crate::wardrobe::{Catalogue, Claim, Set};
use std::collections::{BTreeMap, HashSet};

/// Where the website opens Paddle's checkout for a transaction the service started.
pub const PAY_URL: &str = "https://playhorizon.dev/shop/pay";

/// The founder set's id in an order: the free claim, through the same checkout.
pub const FOUNDER_SET: &str = "founder_set";

/// Whether `claim`, the service's answer, gives the Wardrobe something to show.
pub fn shown(claim: &Claim) -> bool {
    claim.open
        || claim.claimed == Some(true)
        || (claim.starts_at.is_some() && claim.ends_at.is_some())
}

/// Whether the top bar has the Wardrobe page: `HORIZON_COSMETICS=1` asked for it (`forced`), or
/// the service's last answer (`status`) or the account's own (`kept`, from its last visit) shows
/// the founder set. Cosmetics being on in the game (the default) doesn't: the page waits for it.
pub fn in_top_bar(forced: bool, status: Option<&Claim>, kept: Option<&Claim>) -> bool {
    forced || status.is_some_and(shown) || kept.is_some_and(shown)
}

/// The website's page that opens Paddle's checkout for `txn`, or None if `txn` doesn't look like
/// one of Paddle's transaction ids (it goes into a URL).
pub fn pay_url(txn: &str) -> Option<String> {
    let id = txn.strip_prefix("txn_")?;
    (!id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| format!("{PAY_URL}?_ptxn={txn}"))
}

/// A price as the shop writes it, in cents of `currency`: `$4.99`, `€4.99`, `£4.49`,
/// `CHF 4.90`, else `4.99 XYZ`.
pub fn format_price(cents: u32, currency: &str) -> String {
    let amount = format!("{}.{:02}", cents / 100, cents % 100);
    match currency {
        "USD" => format!("${amount}"),
        "EUR" => format!("€{amount}"),
        "GBP" => format!("£{amount}"),
        "CHF" => format!("CHF {amount}"),
        other => format!("{amount} {other}"),
    }
}

/// The currencies every price on sale has, in the service's order.
pub fn currencies(c: &Catalogue) -> Vec<String> {
    let has = |prices: &BTreeMap<String, u32>, cur: &String| prices.contains_key(cur);
    (c.currencies.iter())
        .filter(|cur| {
            c.items
                .iter()
                .all(|i| i.prices.is_empty() || has(&i.prices, cur))
                && c.sets.iter().all(|s| has(&s.prices, cur))
                && c.supporter
                    .as_ref()
                    .is_none_or(|s| has(&s.min, cur) && has(&s.max, cur))
        })
        .cloned()
        .collect()
}

/// The currency a player most likely pays in, from the system's locale (`fr-CH`, `en_GB`...):
/// francs in Switzerland and Liechtenstein, pounds in the UK, euros in the euro area, else
/// dollars (or the first one on offer).
pub fn guess_currency(available: &[String], locale: Option<&str>) -> Option<String> {
    const EURO: &[&str] = &[
        "AT", "BE", "CY", "DE", "EE", "ES", "FI", "FR", "GR", "HR", "IE", "IT", "LT", "LU", "LV",
        "MT", "NL", "PT", "SI", "SK", "AD", "MC", "SM", "VA", "ME", "XK",
    ];
    let region = locale
        .and_then(|l| l.split(['-', '_', '.']).nth(1))
        .map(str::to_ascii_uppercase)
        .unwrap_or_default();
    let wanted = match region.as_str() {
        "CH" | "LI" => "CHF",
        "GB" => "GBP",
        r if EURO.contains(&r) => "EUR",
        _ => "USD",
    };
    (available.iter().find(|c| *c == wanted))
        .or_else(|| available.iter().find(|c| *c == "USD"))
        .or(available.first())
        .cloned()
}

/// `set × unowned / full`, rounded to the cent, with the service's integer math.
pub fn set_share(set: u32, unowned: u32, full: u32) -> u32 {
    if full == 0 {
        return 0;
    }
    let (set, unowned, full) = (u64::from(set), u64::from(unowned), u64::from(full));
    ((set * unowned + full / 2) / full) as u32
}

/// Whether the account owns all of `id`: an item it has, or a set whose every item it has. What
/// it owns shows "Owned" and nothing to buy.
pub fn owns(id: &str, c: &Catalogue, owned: &HashSet<String>) -> bool {
    match c.sets.iter().find(|s| s.id == id) {
        Some(set) => set.items.iter().all(|i| owned.contains(i)),
        None => owned.contains(id),
    }
}

/// What a set costs an account that owns `owned` of its items, and how many those are.
pub fn set_price(
    set: &Set,
    c: &Catalogue,
    owned: &HashSet<String>,
    currency: &str,
) -> (u32, usize) {
    let price = |id: &String| {
        (c.items.iter().find(|i| &i.id == id))
            .and_then(|i| i.prices.get(currency).copied())
            .unwrap_or(0)
    };
    let full: u32 = set.items.iter().map(price).sum();
    let unowned: u32 = set
        .items
        .iter()
        .filter(|i| !owned.contains(*i))
        .map(price)
        .sum();
    let mine = set.items.iter().filter(|i| owned.contains(*i)).count();
    let whole = set.prices.get(currency).copied().unwrap_or(0);
    let amount = if mine == 0 {
        whole
    } else {
        set_share(whole, unowned, full)
    };
    (amount, mine)
}

/// What the player picked, in the order they did: item and set ids, and the Supporter badge's
/// price if they chose one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Basket {
    pub items: Vec<String>,
    pub supporter: Option<u32>,
}

impl Basket {
    /// Puts `id` in, or takes it out if it is in.
    pub fn toggle(&mut self, id: &str) {
        match self.items.iter().position(|i| i == id) {
            Some(at) => {
                self.items.remove(at);
            }
            None => self.items.push(id.to_owned()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.supporter.is_none()
    }

    /// The ids the checkout takes: the basket's, then the Supporter badge's.
    pub fn ids(&self, c: &Catalogue) -> Vec<String> {
        let badge = (self.supporter.is_some())
            .then(|| c.supporter.as_ref().map(|s| s.item.clone()))
            .flatten();
        self.items.iter().cloned().chain(badge).collect()
    }
}

/// Why a line of the basket costs nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// The account owns it (all of a set).
    Owned,
    /// It is inside a set in the same basket.
    InSet,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub id: String,
    pub set: bool,
    pub amount: u32,
    /// For a set: how many of its items the account owns already.
    pub owned: usize,
    pub dropped: Option<Dropped>,
}

/// The basket's lines and their total in `currency`, for an account that owns `owned`. An id the
/// catalogue doesn't sell any more is left out.
pub fn lines(
    b: &Basket,
    c: &Catalogue,
    owned: &HashSet<String>,
    currency: &str,
) -> (Vec<Line>, u32) {
    let in_sets: HashSet<&String> = (b.items.iter())
        .filter_map(|id| c.sets.iter().find(|s| &s.id == id))
        .flat_map(|s| &s.items)
        .collect();
    let mut out = Vec::new();
    for id in &b.items {
        if let Some(set) = c.sets.iter().find(|s| &s.id == id) {
            let (amount, mine) = set_price(set, c, owned, currency);
            out.push(Line {
                id: id.clone(),
                set: true,
                amount,
                owned: mine,
                dropped: (mine == set.items.len()).then_some(Dropped::Owned),
            });
            continue;
        }
        let Some(amount) =
            (c.items.iter().find(|i| &i.id == id)).and_then(|i| i.prices.get(currency))
        else {
            continue;
        };
        let dropped = if owned.contains(id) {
            Some(Dropped::Owned)
        } else {
            in_sets.contains(id).then_some(Dropped::InSet)
        };
        out.push(Line {
            id: id.clone(),
            set: false,
            amount: *amount,
            owned: 0,
            dropped,
        });
    }
    if let (Some(amount), Some(s)) = (b.supporter, &c.supporter) {
        out.push(Line {
            id: s.item.clone(),
            set: false,
            amount,
            owned: 0,
            dropped: owned.contains(&s.item).then_some(Dropped::Owned),
        });
    }
    let total = out
        .iter()
        .filter(|l| l.dropped.is_none())
        .map(|l| l.amount)
        .sum();
    (out, total)
}

/// What the player wears in the Shop's view: what they wear now, each tried item in its own slot
/// over it (the last one tried wins a slot).
pub fn try_on<'a>(
    worn: &BTreeMap<String, String>,
    tried: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, String> {
    let mut out = worn.clone();
    for (slot, id) in tried {
        out.insert(slot.to_owned(), id.to_owned());
    }
    out
}

#[cfg(test)]
mod tests;
