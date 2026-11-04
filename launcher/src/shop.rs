//! Whether the launcher shows the Shop (the founder set to claim, in the Wardrobe page) at all:
//! only when Horizon's cosmetics service says so, whatever the cosmetics switch says. The service
//! decides with its claim window (`GET /v1/shop`): the founder set can be claimed now (the public
//! window, or a preview for an account on its list), the account claimed it, or the public window
//! has dates (the page then says when it opens, or that it is over). With no window set, the
//! launcher shows nothing of it. Nothing in it asks for money: the founder set is free.
use crate::wardrobe::Claim;

/// Whether `claim`, the service's answer, gives the Shop something to show.
pub fn shown(claim: &Claim) -> bool {
    claim.open
        || claim.claimed == Some(true)
        || (claim.starts_at.is_some() && claim.ends_at.is_some())
}

/// Whether the top bar has the Wardrobe page: the cosmetics switch is on, or the service's last
/// answer (`status`) or the account's own (`kept`, from its last visit) shows the Shop.
pub fn in_top_bar(cosmetics: bool, status: Option<&Claim>, kept: Option<&Claim>) -> bool {
    cosmetics || status.is_some_and(shown) || kept.is_some_and(shown)
}

#[cfg(test)]
mod tests;
