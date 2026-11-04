//! Whether the top bar has the Wardrobe page at all: only when Horizon's cosmetics service says
//! there is something to show, whatever the cosmetics switch says. The service decides with its
//! founder-set window (`GET /v1/shop`): the set can be claimed now (the public window, or a preview
//! for an account on its list), the account claimed it, or the public window has dates. With no
//! window set, the launcher shows nothing. The shop itself (browsing, buying, the claim) is on the
//! website, not in the launcher.
use crate::wardrobe::Claim;

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

#[cfg(test)]
mod tests;
