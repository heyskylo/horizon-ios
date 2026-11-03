use super::*;

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

fn me(claim: Claim, owned: &[&str]) -> Me {
    Me {
        uuid: "u".into(),
        name: "Notch".into(),
        owned: owned.iter().map(|s| s.to_string()).collect(),
        equipped: Default::default(),
        visible: true,
        claim,
    }
}

#[test]
fn the_window_follows_the_clock_between_answers() {
    let c = claim(ClaimState::NotYet, false, Some(100), Some(200));
    assert_eq!(
        window(&c, 50),
        Window::NotYet {
            starts_at: Some(100)
        }
    );
    // Opened since the service said "not yet".
    assert_eq!(window(&c, 100), Window::Open { ends_at: Some(200) });
    let c = claim(ClaimState::Open, true, Some(100), Some(200));
    assert_eq!(window(&c, 199), Window::Open { ends_at: Some(200) });
    // The end is excluded: no claim button at the very second it ends.
    assert_eq!(
        window(&c, 200),
        Window::Closed {
            ended_at: Some(200)
        }
    );
    // The service's "closed" stands even if this computer's clock is behind.
    let c = claim(ClaimState::Closed, false, Some(100), Some(200));
    assert_eq!(
        window(&c, 150),
        Window::Closed {
            ended_at: Some(200)
        }
    );
    // A preview: open with no dates, so no time left to count.
    let c = claim(ClaimState::Open, true, None, None);
    assert_eq!(window(&c, 150), Window::Open { ends_at: None });
    // Nothing set and not open: not yet, no date to show.
    let c = claim(ClaimState::NotYet, false, None, None);
    assert_eq!(window(&c, 150), Window::NotYet { starts_at: None });
}

#[test]
fn the_set_is_owned_once_claimed_or_every_item_is_owned() {
    let open = claim(ClaimState::Open, true, Some(100), Some(200));
    assert!(!owns_set(&me(open.clone(), &[])));
    assert!(!owns_set(&me(open.clone(), &["visor"])));
    assert!(owns_set(&me(open.clone(), &["visor", "sunlet"])));
    let mut claimed = open;
    claimed.claimed = Some(true);
    assert!(owns_set(&me(claimed, &[])));
}

#[test]
fn time_left_reads_in_its_two_largest_units() {
    const M: i64 = 60;
    const H: i64 = 3600;
    const D: i64 = 86_400;
    assert_eq!(span(0, 30), "less than a minute");
    assert_eq!(span(0, 8 * M + 59), "8 min");
    assert_eq!(span(0, 3 * H + 20 * M), "3 h 20 min");
    assert_eq!(span(0, D), "1 day");
    assert_eq!(span(0, D + 4 * H + 5 * M), "1 day 4 h");
    assert_eq!(span(0, 2 * D + 5 * H), "2 days 5 h");
    assert_eq!(span(0, 2 * D), "2 days");
    assert_eq!(span(0, 12 * D + 23 * H), "12 days");
    // Past the end: nothing left, never a negative count.
    assert_eq!(span(500, 100), "less than a minute");
}

#[test]
fn a_date_reads_day_month_year() {
    assert_eq!(date(2027, 3, 31), "31 March 2027");
    assert_eq!(date(2026, 12, 1), "1 December 2026");
}
