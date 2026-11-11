//! The `count` wrapper of an item predicate (`ItemPredicateArgument`): both
//! `count=` and `count~` read the live stack count.

use horizon_world::nbt::Tag;

/// `MinMaxBounds.Ints.matches` on the stack count: an exact Int, or a
/// compound with optional Int `min` and `max`.
pub(super) fn matches(count: impl Into<i64>, want: &Tag) -> bool {
    let n = count.into();
    match want {
        Tag::Compound(_) => {
            let get = |k: &str| match want.get(k) {
                Some(Tag::Int(i)) => Some(i64::from(*i)),
                _ => None,
            };
            get("min").is_none_or(|m| n >= m) && get("max").is_none_or(|m| n <= m)
        }
        Tag::Int(i) => n == i64::from(*i),
        _ => false,
    }
}
