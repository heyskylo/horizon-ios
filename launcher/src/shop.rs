//! Whether the launcher shows the Shop (the founder set to claim, in the Wardrobe page) at all:
//! only when Horizon's cosmetics service says so, whatever the cosmetics switch says. The service
//! decides with its claim window (`GET /v1/shop`): the founder set can be claimed now (the public
//! window, or a preview for an account on its list), the account claimed it, or the public window
//! has dates (the page then says when it opens, or that it is over). With no window set, the
//! launcher shows nothing of it. Nothing in it asks for money: the founder set is free.
//!
//! Also the Shop tab's rules apart from its drawing (`wardrobe_page/shop.rs`): where the claim
//! window stands, and its dates and the time left in the player's words.
use crate::i18n::{tr, trf};
use crate::wardrobe::{Claim, ClaimState, Me};

/// Whether `claim`, the service's answer, gives the Shop something to show.
pub fn shown(claim: &Claim) -> bool {
    claim.open
        || claim.claimed == Some(true)
        || (claim.starts_at.is_some() && claim.ends_at.is_some())
}

/// Whether the top bar has the Wardrobe page: `HORIZON_COSMETICS=1` asked for it (`forced`), or
/// the service's last answer (`status`) or the account's own (`kept`, from its last visit) shows
/// the Shop. Cosmetics being on in the game (the default) doesn't: the page waits for the Shop.
pub fn in_top_bar(forced: bool, status: Option<&Claim>, kept: Option<&Claim>) -> bool {
    forced || status.is_some_and(shown) || kept.is_some_and(shown)
}

/// Where the founder set's window stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// It opens at `starts_at` (None: no date yet).
    NotYet { starts_at: Option<i64> },
    /// Claimable until `ends_at` (excluded; None: a preview with no end date).
    Open { ends_at: Option<i64> },
    /// Retired at `ended_at`: never offered again, never sold.
    Closed { ended_at: Option<i64> },
}

/// The service's word on the window, moved on by the clock: a window that ended since the last
/// answer reads as over at once, one that opened reads as open (the service still decides each
/// claim). `now` is on the service's time (`wardrobe::service_now`), so a fast clock doesn't end a
/// window early. A state this launcher doesn't know offers nothing and says nothing of the end.
pub fn window(claim: &Claim, now: i64) -> Window {
    if claim.state == ClaimState::Unknown {
        return Window::NotYet { starts_at: None };
    }
    let ended = claim.ends_at.is_some_and(|end| now >= end);
    let started = claim.starts_at.is_some_and(|start| now >= start);
    let closed = Window::Closed {
        ended_at: claim.ends_at,
    };
    if ended || claim.state == ClaimState::Closed && !claim.open {
        return closed;
    }
    if claim.open || claim.state == ClaimState::Open || started && claim.ends_at.is_some() {
        return Window::Open {
            ends_at: claim.ends_at,
        };
    }
    Window::NotYet {
        starts_at: claim.starts_at,
    }
}

/// Whether the account has the founder set: it claimed it, or owns every item of it.
pub fn owns_set(me: &Me) -> bool {
    me.claim.claimed == Some(true)
        || (!me.claim.items.is_empty() && me.claim.items.iter().all(|i| me.owned.contains(i)))
}

/// The time from `now` to `to`, the largest two units: "1 day 4 h", "3 h 20 min", "8 min"; under a
/// minute: "less than a minute". From three days up, whole days to the nearest one, so a window that
/// just opened for seven days reads "7 days", not "6 days".
pub fn span(now: i64, to: i64) -> String {
    let secs = (to - now).max(0);
    let (days, hours, mins) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    let n = |v: i64| v.to_string();
    match (days, hours, mins) {
        (0, 0, 0) => tr("less than a minute").into(),
        (0, 0, m) => trf("{m} min", &[("m", &n(m))]),
        (0, h, m) => trf("{h} h {m} min", &[("h", &n(h)), ("m", &n(m))]),
        (1, 0, _) => tr("1 day").into(),
        (1, h, _) => trf("1 day {h} h", &[("h", &n(h))]),
        (d, _, _) if d >= 3 => trf("{d} days", &[("d", &n((secs + 43_200) / 86_400))]),
        (d, 0, _) => trf("{d} days", &[("d", &n(d))]),
        (d, h, _) => trf("{d} days {h} h", &[("d", &n(d)), ("h", &n(h))]),
    }
}

/// A moment in the player's words, in their time zone: "31 March 2027", with the time when it
/// isn't midnight ("31 March 2027, 18:00"). An end at midnight reads as the day before it, the
/// last day it is open ("until 31 March" for an end at 1 April 00:00).
pub fn until(secs: i64) -> String {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let Some(at) = Local.timestamp_opt(secs, 0).single() else {
        return String::new();
    };
    if at.hour() == 0 && at.minute() == 0 {
        if let Some(day) = Local.timestamp_opt(secs - 1, 0).single() {
            return date(day.year(), day.month(), day.day());
        }
    }
    trf(
        "{date}, {time}",
        &[
            ("date", &date(at.year(), at.month(), at.day())),
            ("time", &at.format("%H:%M").to_string()),
        ],
    )
}

/// The day a window starts on, with its time when it isn't midnight.
pub fn from(secs: i64) -> String {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let Some(at) = Local.timestamp_opt(secs, 0).single() else {
        return String::new();
    };
    let day = date(at.year(), at.month(), at.day());
    if at.hour() == 0 && at.minute() == 0 {
        return day;
    }
    trf(
        "{date}, {time}",
        &[("date", &day), ("time", &at.format("%H:%M").to_string())],
    )
}

/// "31 March 2027" in the launcher's language.
pub fn date(year: i32, month: u32, day: u32) -> String {
    let month = match month {
        1 => tr("January"),
        2 => tr("February"),
        3 => tr("March"),
        4 => tr("April"),
        5 => tr("May"),
        6 => tr("June"),
        7 => tr("July"),
        8 => tr("August"),
        9 => tr("September"),
        10 => tr("October"),
        11 => tr("November"),
        _ => tr("December"),
    };
    trf(
        "{day} {month} {year}",
        &[
            ("day", &day.to_string()),
            ("month", month),
            ("year", &year.to_string()),
        ],
    )
}

#[cfg(test)]
mod tests;
