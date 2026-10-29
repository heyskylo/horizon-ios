//! Resource identifiers (`Identifier`).

/// Identifier.parse: `ns:path` or `path`, validated.
pub fn parse_identifier(raw: &str) -> Option<String> {
    let (ns, path) = match raw.split_once(':') {
        Some(("", path)) => ("minecraft", path),
        Some((ns, path)) => (ns, path),
        None => ("minecraft", raw),
    };
    let ns_ok = ns != ".."
        && ns
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    let path_ok = path.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-' | '/')
    });
    (ns_ok && path_ok).then(|| format!("{ns}:{path}"))
}

/// `Identifier.CODEC` parsing an entity id: lowercase namespace and path
/// characters, `minecraft` when the namespace is left out.
pub fn normalize_id(s: &str) -> Option<String> {
    let (ns, path) = match s.split_once(':') {
        Some((ns, path)) => (if ns.is_empty() { "minecraft" } else { ns }, path),
        None => ("minecraft", s),
    };
    let ok = |c: char, slash: bool| {
        c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || matches!(c, '_' | '-' | '.')
            || slash && c == '/'
    };
    (ns.chars().all(|c| ok(c, false)) && path.chars().all(|c| ok(c, true)))
        .then(|| format!("{ns}:{path}"))
}
