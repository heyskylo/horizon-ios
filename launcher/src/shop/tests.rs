use super::*;
use crate::wardrobe::ClaimState;

/// The service's answer.
fn claim(state: ClaimState, open: bool, starts_at: Option<i64>, ends_at: Option<i64>) -> Claim {
    Claim {
        state,
        items: vec!["founder_visor".into()],
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
