//! The folder a new world gets: `WorldCreationUiState.findResultFolder` through
//! `FileUtil.findAvailableName`. Lengths are Java's, in UTF-16 units.

/// `SharedConstants.ILLEGAL_FILE_CHARACTERS`, in its order.
const ILLEGAL_FILE_CHARACTERS: [char; 15] = [
    '/', '\n', '\r', '\t', '\0', '\u{c}', '`', '?', '*', '\\', '<', '>', '|', '"', ':',
];

/// `FileUtil.MAX_FILE_NAME`.
const MAX_FILE_NAME: usize = 255;

/// The names `FileUtil.RESERVED_WINDOWS_FILENAMES` refuses once `sanitizeName` has turned every
/// dot into `_` (its `.*\.` and `(?:\..*)?` parts can no longer match).
const RESERVED: [&str; 24] = [
    "COM", "CLOCK$", "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `findResultFolder(name)`: the name trimmed as `String.trim` does, `new_world` (the translated
/// `selectWorld.newWorld`) when that leaves nothing, made free of the folders `taken` says exist.
/// A name whose count can't be read (`Mine ()`) falls back to `World`, as the method's catch does.
pub(super) fn result_folder(name: &str, new_world: &str, taken: impl Fn(&str) -> bool) -> String {
    let name = name.trim_matches(|c: char| c <= ' ');
    let name = if name.is_empty() { new_world } else { name };
    available_name(name, &taken)
        .or_else(|| available_name("World", &taken))
        .unwrap_or_else(|| "World".to_owned())
}

/// `FileUtil.sanitizeName`.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if ILLEGAL_FILE_CHARACTERS.contains(&c) || matches!(c, '.' | '/' | '"') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// `FileUtil.COPY_COUNTER_PATTERN`, `(?<name>.*) \((?<count>\d*)\)`, matched whole: the name and
/// the digits. `.` matches no line terminator, `\d` only ASCII digits.
fn copy_counter(name: &str) -> Option<(&str, &str)> {
    let inner = name.strip_suffix(')')?;
    let digits = inner.len() - inner.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (rest, digits) = inner.split_at(inner.len() - digits);
    let base = rest.strip_suffix(" (")?;
    let line_end = |c: char| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}');
    (!base.contains(line_end)).then_some((base, digits))
}

fn java_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `String.substring(0, n)` in UTF-16 units (a pair cut in two becomes U+FFFD).
fn java_prefix(s: &str, n: usize) -> String {
    String::from_utf16_lossy(&s.encode_utf16().take(n).collect::<Vec<_>>())
}

/// `FileUtil.findAvailableName(folder, name, "")`: None where Java's `Integer.parseInt` of the
/// count throws.
fn available_name(name: &str, taken: &impl Fn(&str) -> bool) -> Option<String> {
    let mut name = sanitize_name(name);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
        name = format!("_{name}_");
    }
    let mut count = 0i32;
    if let Some((base, digits)) = copy_counter(&name) {
        count = digits.parse().ok()?;
        name = base.to_owned();
    }
    if java_len(&name) > MAX_FILE_NAME {
        name = java_prefix(&name, MAX_FILE_NAME);
    }
    loop {
        let mut candidate = name.clone();
        if count != 0 {
            let counter = format!(" ({count})");
            let room = MAX_FILE_NAME - counter.len();
            if java_len(&candidate) > room {
                candidate = java_prefix(&candidate, room);
            }
            candidate.push_str(&counter);
        }
        if !taken(&candidate) {
            return Some(candidate);
        }
        count = count.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::result_folder;

    fn folder(name: &str, taken: &[&str]) -> String {
        result_folder(name, "New World", |f| taken.contains(&f))
    }

    #[test]
    fn a_new_worlds_folder_is_named_as_vanilla_names_it() {
        // A blank name takes the translated "New World", trimmed as String.trim does.
        assert_eq!(folder("", &[]), "New World");
        assert_eq!(folder(" \t ", &[]), "New World");
        assert_eq!(folder("  Base  ", &[]), "Base");
        // sanitizeName: only ILLEGAL_FILE_CHARACTERS and . / " become _.
        assert_eq!(folder("My.World/Test", &[]), "My_World_Test");
        assert_eq!(folder("Bob's world! #1", &[]), "Bob's world! #1");
        assert_eq!(folder("a:b*c?d", &[]), "a_b_c_d");
        // A name Windows reserves is wrapped in underscores (isPathPartPortable).
        assert_eq!(folder("con", &[]), "_con_");
        assert_eq!(folder("LPT1", &[]), "_LPT1_");
        // A name already taken counts up from 1; a name that ends in a count goes on from it.
        assert_eq!(folder("Hello", &["Hello"]), "Hello (1)");
        assert_eq!(folder("Hello", &["Hello", "Hello (1)"]), "Hello (2)");
        assert_eq!(folder("World (3)", &[]), "World (3)");
        assert_eq!(folder("World (3)", &["World (3)"]), "World (4)");
        // An empty count can't be parsed: findResultFolder falls back to "World".
        assert_eq!(folder("Mine ()", &[]), "World");
        // At most 255 characters, the count included.
        let long = "a".repeat(300);
        assert_eq!(folder(&long, &[]), "a".repeat(255));
        let first = "a".repeat(255);
        assert_eq!(
            folder(&long, &[first.as_str()]),
            format!("{} (1)", "a".repeat(251))
        );
    }
}
