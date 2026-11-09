//! Moderation durations: `/tempmute` and `/tempban`'s argument, and how long a mute or ban lasts when the
//! server tells it.

const MINUTE: i64 = 60_000;

/// `/tempmute`'s duration in milliseconds: a positive whole number and a unit, `s`, `m`, `h`, `d` or
/// `w` (`10m`, `2h`, `1d`).
pub fn parse_duration(text: &str) -> Option<i64> {
    let unit = match text.chars().last()? {
        's' => 1_000,
        'm' => MINUTE,
        'h' => 60 * MINUTE,
        'd' => 24 * 60 * MINUTE,
        'w' => 7 * 24 * 60 * MINUTE,
        _ => return None,
    };
    let count: i64 = text[..text.len() - 1].parse().ok()?;
    (count > 0).then(|| count.checked_mul(unit)).flatten()
}

/// The unit used when presenting a moderation duration to a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationUnit {
    Minute,
    Hour,
    Day,
}

/// A duration's rounded count and unit, shared by logs and translated replies.
pub fn duration_parts(millis: i64) -> (i64, DurationUnit) {
    let (unit, kind) = if millis <= 60 * MINUTE {
        (MINUTE, DurationUnit::Minute)
    } else if millis <= 48 * 60 * MINUTE {
        (60 * MINUTE, DurationUnit::Hour)
    } else {
        (24 * 60 * MINUTE, DurationUnit::Day)
    };
    (
        millis.div_euclid(unit) + i64::from(millis.rem_euclid(unit) > 0),
        kind,
    )
}

/// `millis` for logs, rounded up: minutes up to an hour, hours up to two days, then days.
pub fn duration_text(millis: i64) -> String {
    let (count, unit) = duration_parts(millis);
    let unit = match unit {
        DurationUnit::Minute => "minute",
        DurationUnit::Hour => "hour",
        DurationUnit::Day => "day",
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
