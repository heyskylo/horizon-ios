//! `StatePropertiesPredicate.RangedMatcher`: parse through the actual legal domain.
//!
//! The pinned vanilla registry's property kinds and all enum comparisons are checked
//! independently. Direction's possible-values order differs from Comparable order;
//! other vanilla enums preserve it. This does not establish modded/future enum order.
use horizon_core::{geometry::Direction, text::parse_java_integer};
use serde_json::{Map, Value};

fn direction(value: &str) -> Option<Direction> {
    Some(match value {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    })
}

/// StatePropertiesPredicate.RangedMatcher.match: inclusive typed, legal-domain comparisons.
pub(super) fn matches(bounds: &Map<String, Value>, actual: &str, domain: &[String]) -> bool {
    if domain.is_empty() || !domain.iter().any(|v| v == actual) {
        return false;
    }
    let boolean = domain.len() == 2
        && domain.iter().any(|v| v == "true")
        && domain.iter().any(|v| v == "false");
    // The existing feature-codec integer-family inference, independently checked
    // against actual IntegerProperty kinds for every pinned registered property.
    let integer = domain.iter().all(|v| {
        !v.is_empty()
            && v.trim_start_matches('-')
                .bytes()
                .all(|b| b.is_ascii_digit())
    });
    let directional = domain.iter().all(|v| direction(v).is_some());
    let rank = |value: &str| -> Option<i32> {
        if integer {
            let value = parse_java_integer(value)?;
            return domain
                .iter()
                .any(|v| *v == value.to_string())
                .then_some(value);
        }
        // Membership precedes comparison, including for filtered Direction domains.
        let index = domain.iter().position(|v| v == value)?;
        if boolean {
            Some(i32::from(value == "true"))
        } else if directional {
            Some(i32::from(direction(value)? as u8))
        } else {
            // Only the separately validated remaining pinned vanilla enums.
            i32::try_from(index).ok()
        }
    };
    let Some(actual) = rank(actual) else {
        return false;
    };
    for (key, lower) in [("min", true), ("max", false)] {
        if let Some(bound) = bounds.get(key) {
            let Some(bound) = bound.as_str().and_then(rank) else {
                return false;
            };
            if (lower && actual < bound) || (!lower && actual > bound) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(domain: &[&str], actual: &str, range: Value, expected: bool) {
        let domain: Vec<_> = domain.iter().map(|v| (*v).to_owned()).collect();
        assert_eq!(
            matches(range.as_object().unwrap(), actual, &domain),
            expected,
            "actual={actual}, range={range}, domain={domain:?}"
        );
    }
    #[test]
    fn boolean_uses_comparable_not_values_order() {
        let d = ["true", "false"];
        check(&d, "false", json!({"max":"false"}), true);
        check(&d, "true", json!({"max":"false"}), false);
        check(&d, "true", json!({"min":"true"}), true);
        check(&d, "false", json!({"min":"true"}), false);
        check(&d, "true", json!({"min":"true","max":"false"}), false);
        check(&d, "true", json!({"min":"True"}), false);
    }
    #[test]
    fn integer_bounds_parse_then_require_legal_values() {
        let d = ["0", "1", "2", "3", "4", "5", "6", "7"];
        check(&d, "5", json!({"min":"2","max":"06"}), true);
        check(&d, "7", json!({"min":"2","max":"06"}), false);
        check(&d, "5", json!({"min":"6","max":"2"}), false);
        for bound in ["10", "-1", "2147483648", "1e0", " 2"] {
            check(&d, "5", json!({"min":bound}), false);
        }
        check(
            &["-2", "-1", "0"],
            "-1",
            json!({"min":"-02","max":"0"}),
            true,
        );
    }
    #[test]
    fn full_direction_compares_existing_native_enum() {
        // Actual six-facing Property values; west follows south in Comparable.
        let d = ["north", "east", "south", "west", "up", "down"];
        for actual in d {
            check(
                &d,
                actual,
                json!({"min":"north","max":"west"}),
                matches!(actual, "north" | "south" | "west"),
            );
            check(&d, actual, json!({"min":"up","max":"down"}), false);
        }
    }
    #[test]
    fn filtered_direction_membership_is_not_widened() {
        let d = ["up", "down"];
        check(&d, "down", json!({"max":"down"}), true);
        check(&d, "up", json!({"max":"down"}), false);
        check(&d, "up", json!({"min":"up"}), true);
        check(&d, "down", json!({"min":"up"}), false);
        check(&d, "up", json!({"min":"north"}), false);
        check(
            &["north", "south", "west", "east"],
            "north",
            json!({"min":"down"}),
            false,
        );
    }
    #[test]
    fn other_enum_preserves_validated_nonlexical_order() {
        let d = ["single", "left", "right"];
        check(&d, "single", json!({"min":"left"}), false);
        check(&d, "right", json!({"min":"left"}), true);
        check(&d, "single", json!({"min":"unknown"}), false);
    }
    #[test]
    fn unavailable_or_unknown_values_and_present_invalid_bounds_fail() {
        check(&[], "north", json!({}), false);
        check(&["north", "south"], "missing", json!({}), false);
        check(&["north", "south"], "north", json!({}), true);
        for bound in [json!(null), json!(2), json!(false), json!({})] {
            check(&["north", "south"], "north", json!({"min":bound}), false);
        }
    }
}
