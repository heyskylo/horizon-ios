use super::*;
use crate::wardrobe::ClaimState;
use std::collections::{BTreeMap, HashSet};

/// The service's answer.
fn claim(state: ClaimState, open: bool, starts_at: Option<i64>, ends_at: Option<i64>) -> Claim {
    Claim {
        state,
        items: vec!["visor".into(), "sunlet".into()],
        starts_at,
        ends_at,
        claimed: None,
        open,
        preview: false,
        one_click: false,
    }
}

#[test]
fn the_shop_shows_only_when_the_service_has_something_to_show() {
    // Production today: no window, not open.
    let unset = claim(ClaimState::NotYet, false, None, None);
    assert!(!shown(&unset));
    // One date alone is not a window.
    assert!(!shown(&claim(ClaimState::NotYet, false, Some(100), None)));
    // A window with dates: before (when it opens), during, after (that it is over).
    assert!(shown(&claim(
        ClaimState::NotYet,
        false,
        Some(100),
        Some(200)
    )));
    assert!(shown(&claim(ClaimState::Open, true, Some(100), Some(200))));
    assert!(shown(&claim(
        ClaimState::Closed,
        false,
        Some(100),
        Some(200)
    )));
    // A preview for this account: open with no dates.
    let mut preview = claim(ClaimState::Open, true, None, None);
    preview.preview = true;
    assert!(shown(&preview));
    // The account claimed it: it keeps seeing what it owns.
    let mut claimed = unset;
    claimed.claimed = Some(true);
    assert!(shown(&claimed));
}

#[test]
fn the_wardrobe_entry_follows_the_service_not_the_cosmetics_switch() {
    let unset = claim(ClaimState::NotYet, false, None, None);
    let open = claim(ClaimState::Open, true, Some(100), Some(200));
    // Switch off (the default): the service alone decides.
    assert!(!in_top_bar(false, None, None));
    assert!(!in_top_bar(false, Some(&unset), Some(&unset)));
    assert!(in_top_bar(false, Some(&open), None));
    // The account's own answer from its last visit counts too (a preview, a claim).
    assert!(in_top_bar(false, Some(&unset), Some(&open)));
    // Switch on: the Wardrobe is there as before.
    assert!(in_top_bar(true, Some(&unset), None));
}

/// A catalogue in the service's words: three items in dollars and francs, a set of the first
/// two, the Supporter badge.
fn catalogue() -> Catalogue {
    serde_json::from_value(serde_json::json!({
        "claim": { "state": "closed", "items": [] },
        "currencies": ["USD", "EUR", "CHF"],
        "items": [
            { "id": "lantern", "hash": "1", "slot": "head", "names": {}, "prices": { "USD": 499, "CHF": 490 } },
            { "id": "scarf", "hash": "2", "slot": "torso", "names": {}, "prices": { "USD": 299, "CHF": 290 } },
            { "id": "moth", "hash": "3", "slot": "shoulder", "names": {}, "prices": { "USD": 199, "CHF": 190 } },
            { "id": "supporter_tag", "hash": "4", "slot": "name_tag", "names": {} },
            { "id": "old_hat", "hash": "5", "slot": "head", "names": {} }
        ],
        "sets": [{ "id": "hearth", "names": {}, "items": ["lantern", "scarf"], "prices": { "USD": 599, "CHF": 590 } }],
        "supporter": { "item": "supporter_tag", "min": { "USD": 499, "CHF": 490 }, "max": { "USD": 10000, "CHF": 9800 } }
    }))
    .unwrap()
}

fn owns(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn basket(ids: &[&str]) -> Basket {
    Basket {
        items: ids.iter().map(|s| s.to_string()).collect(),
        supporter: None,
    }
}

#[test]
fn an_item_inside_a_set_in_the_basket_is_not_charged_twice() {
    let (got, total) = lines(
        &basket(&["lantern", "hearth", "moth"]),
        &catalogue(),
        &owns(&[]),
        "USD",
    );
    let dropped: Vec<_> = got.iter().map(|l| (l.id.as_str(), l.dropped)).collect();
    assert_eq!(
        dropped,
        [
            ("lantern", Some(Dropped::InSet)),
            ("hearth", None),
            ("moth", None)
        ]
    );
    // The set's 5.99 and the moth's 1.99.
    assert_eq!(total, 798);
}

#[test]
fn a_set_partly_owned_costs_its_share_of_what_is_not_owned() {
    // Owning the scarf (2.99 of the 7.98 apart): 5.99 × 4.99 / 7.98 = 3.7456 → 3.75.
    let (got, total) = lines(&basket(&["hearth"]), &catalogue(), &owns(&["scarf"]), "USD");
    assert_eq!(
        (got[0].amount, got[0].owned, got[0].dropped),
        (375, 1, None)
    );
    assert_eq!(total, 375);
    // In francs: 5.90 × 4.90 / 7.80 = 3.7064 → 3.71.
    assert_eq!(lines_total(&["hearth"], &["scarf"], "CHF"), 371);
    // All of it owned: nothing to pay.
    let all = owns(&["scarf", "lantern"]);
    let (got, total) = lines(&basket(&["hearth"]), &catalogue(), &all, "USD");
    assert_eq!((got[0].dropped, total), (Some(Dropped::Owned), 0));
}

#[test]
fn a_set_is_owned_only_with_every_item() {
    let c = catalogue();
    assert!(!super::owns("hearth", &c, &owns(&["scarf"])));
    assert!(super::owns("hearth", &c, &owns(&["scarf", "lantern"])));
    assert!(super::owns("moth", &c, &owns(&["moth"])));
    assert!(!super::owns("moth", &c, &owns(&["hearth"])));
}

fn lines_total(ids: &[&str], owned: &[&str], currency: &str) -> u32 {
    lines(&basket(ids), &catalogue(), &owns(owned), currency).1
}

#[test]
fn owned_items_and_items_not_on_sale_cost_nothing() {
    assert_eq!(lines_total(&["moth", "lantern"], &["moth"], "USD"), 499);
    // Not priced: not sold, so not in the basket's lines at all.
    let (got, total) = lines(&basket(&["old_hat"]), &catalogue(), &owns(&[]), "USD");
    assert_eq!((got.len(), total), (0, 0));
}

#[test]
fn the_supporter_badge_costs_what_the_player_chose_once() {
    let c = catalogue();
    let mut b = basket(&["moth"]);
    b.supporter = Some(1200);
    assert_eq!(lines(&b, &c, &owns(&[]), "USD").1, 1399);
    assert_eq!(b.ids(&c), ["moth", "supporter_tag"]);
    // Already a supporter: the badge isn't charged again.
    assert_eq!(lines(&b, &c, &owns(&["supporter_tag"]), "USD").1, 199);
}

#[test]
fn a_basket_toggles_its_items() {
    let mut b = Basket::default();
    b.toggle("moth");
    b.toggle("hearth");
    b.toggle("moth");
    assert_eq!(b.items, ["hearth"]);
    assert!(!b.is_empty());
}

#[test]
fn only_currencies_every_price_has_are_offered() {
    // No item, set or badge has euros.
    assert_eq!(currencies(&catalogue()), ["USD", "CHF"]);
}

#[test]
fn the_currency_follows_the_system_locale() {
    let all: Vec<String> = ["USD", "EUR", "CHF", "GBP"].map(String::from).into();
    let guess = |l: Option<&str>| guess_currency(&all, l).unwrap();
    assert_eq!(guess(Some("fr-CH")), "CHF");
    assert_eq!(guess(Some("en_GB.UTF-8")), "GBP");
    assert_eq!(guess(Some("de-AT")), "EUR");
    assert_eq!(guess(Some("en-US")), "USD");
    assert_eq!(guess(Some("fr")), "USD");
    assert_eq!(guess(None), "USD");
    // Francs not offered: dollars.
    let some: Vec<String> = ["USD", "EUR"].map(String::from).into();
    assert_eq!(guess_currency(&some, Some("de-CH")).unwrap(), "USD");
    assert_eq!(guess_currency(&[], Some("de-CH")), None);
}

#[test]
fn prices_read_as_the_website_writes_them() {
    assert_eq!(format_price(499, "USD"), "$4.99");
    assert_eq!(format_price(490, "CHF"), "CHF 4.90");
    assert_eq!(format_price(5, "EUR"), "€0.05");
    assert_eq!(format_price(1200, "GBP"), "£12.00");
}

#[test]
fn only_a_paddle_transaction_id_goes_into_the_pay_link() {
    assert_eq!(
        pay_url("txn_01k9abc").as_deref(),
        Some("https://playhorizon.dev/shop/pay?_ptxn=txn_01k9abc")
    );
    assert_eq!(pay_url("txn_"), None);
    assert_eq!(pay_url("txn_01&next=evil"), None);
    assert_eq!(pay_url("ctm_01k9abc"), None);
}

#[test]
fn trying_an_item_on_changes_its_slot_only() {
    let worn: BTreeMap<String, String> = [("head", "visor"), ("back", "pack")]
        .map(|(a, b)| (a.into(), b.into()))
        .into();
    let tried = try_on(&worn, [("head", "lantern"), ("shoulder", "moth")]);
    let tried: Vec<_> = tried
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        tried,
        [("back", "pack"), ("head", "lantern"), ("shoulder", "moth")]
    );
}

#[test]
fn a_catalogue_from_before_the_shop_still_reads() {
    let old: Catalogue = serde_json::from_value(serde_json::json!({
        "claim": { "state": "open", "items": ["visor"], "open": true },
        "items": [{ "id": "visor", "hash": "1", "slot": "head", "names": {} }]
    }))
    .unwrap();
    assert!(old.sets.is_empty() && old.supporter.is_none() && old.items[0].prices.is_empty());
    assert_eq!(currencies(&old), Vec::<String>::new());
}
