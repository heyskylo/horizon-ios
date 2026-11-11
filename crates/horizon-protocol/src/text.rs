//! Chat components as sent on the network (ComponentSerialization in NBT form)
//! flattened to display strings, with `translate` keys resolved through a
//! language table (en_us.json) and Java `String.format` style arguments.
use horizon_world::nbt::{JavaString, Tag};
use std::collections::HashMap;

/// A language table: translation key -> pattern.
pub type Language = HashMap<String, String>;

fn string(tag: &Tag) -> Option<String> {
    match tag {
        Tag::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn get<'t>(tag: &'t Tag, key: &str) -> Option<&'t Tag> {
    match tag {
        Tag::Compound(map) => map.get(&JavaString::from(key)),
        _ => None,
    }
}

/// The longest text [`plain`] and [`format_java`] build; the rest is dropped. A disconnect screen
/// needs a few hundred bytes, and a server's `fallback` template is any text it likes: one that
/// repeats `%1$s` thousands of times, with an argument built the same way a few levels down,
/// asked for terabytes, and the client aborted.
pub const MAX_TEXT: usize = 1 << 20;

/// `s` appended to `out` as far as [`MAX_TEXT`] allows, cut at a character boundary.
pub fn push_capped(out: &mut String, s: &str) {
    let room = MAX_TEXT.saturating_sub(out.len());
    if s.len() <= room {
        out.push_str(s);
    } else {
        let mut end = room;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&s[..end]);
    }
}

/// Flatten a component to text, at most [`MAX_TEXT`] bytes of it.
pub fn plain(tag: &Tag, lang: &Language) -> String {
    let mut out = String::new();
    append(tag, lang, &mut out, 0);
    out
}
fn append(tag: &Tag, lang: &Language, out: &mut String, depth: usize) {
    if depth > 32 || out.len() >= MAX_TEXT {
        return;
    }
    match tag {
        Tag::String(s) => push_capped(out, &s.to_string_lossy()),
        Tag::Byte(v) => push_capped(out, &v.to_string()),
        Tag::Short(v) => push_capped(out, &v.to_string()),
        Tag::Int(v) => push_capped(out, &v.to_string()),
        Tag::Long(v) => push_capped(out, &v.to_string()),
        Tag::Float(v) => push_capped(out, &f32::from_bits(*v).to_string()),
        Tag::Double(v) => push_capped(out, &f64::from_bits(*v).to_string()),
        // A list is its first element with the rest appended as siblings.
        Tag::List(items) => {
            for item in items {
                append(item, lang, out, depth + 1);
            }
        }
        Tag::Compound(_) => {
            if let Some(text) = get(tag, "text").and_then(string) {
                push_capped(out, &text);
            } else if let Some(value) = get(tag, "") {
                // NbtOps' wrapper for a bare value in a mixed list (a
                // translation's number and string arguments).
                append(value, lang, out, depth + 1);
            } else if let Some(key) = get(tag, "translate").and_then(string) {
                let args: Vec<String> = match get(tag, "with") {
                    Some(Tag::List(args)) => args
                        .iter()
                        .map(|a| {
                            let mut s = String::new();
                            append(a, lang, &mut s, depth + 1);
                            s
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let pattern = lang
                    .get(&key)
                    .cloned()
                    .or_else(|| get(tag, "fallback").and_then(string))
                    .unwrap_or_else(|| key.clone());
                push_capped(out, &format_java(&pattern, &args));
            } else if let Some(key) = get(tag, "keybind").and_then(string) {
                push_capped(out, lang.get(&key).map_or(key.as_str(), String::as_str));
            } else if let Some(selector) = get(tag, "selector").and_then(string) {
                push_capped(out, &selector);
            } else if let Some(score) = get(tag, "score")
                && let Some(name) = get(score, "name").and_then(string)
            {
                push_capped(out, &name);
            }
            if let Some(Tag::List(extra)) = get(tag, "extra") {
                for e in extra {
                    append(e, lang, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// TranslatableContents.decomposeTemplate: `%s`, `%n$s` and `%%`, at most [`MAX_TEXT`] bytes.
pub fn format_java(pattern: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut next = 0usize;
    let mut chars = pattern.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if out.len() >= MAX_TEXT {
            break;
        }
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut digits = String::new();
        while let Some(&(_, d)) = chars.peek() {
            if d.is_ascii_digit() {
                digits.push(d);
                chars.next();
            } else {
                break;
            }
        }
        match chars.next() {
            Some((_, '%')) if digits.is_empty() => out.push('%'),
            Some((_, '$')) if !digits.is_empty() => {
                if chars.next().map(|(_, c)| c) == Some('s') {
                    let i = digits.parse::<usize>().unwrap_or(1).saturating_sub(1);
                    push_capped(&mut out, args.get(i).map_or("", String::as_str));
                }
            }
            Some((_, 's')) if digits.is_empty() => {
                push_capped(&mut out, args.get(next).map_or("", String::as_str));
                next += 1;
            }
            Some((_, other)) => {
                out.push('%');
                push_capped(&mut out, &digits);
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// A literal text component.
pub fn literal(text: &str) -> Tag {
    Tag::String(JavaString::from(text))
}
/// A translatable component with literal arguments.
pub fn translatable(key: &str, args: &[&str]) -> Tag {
    let mut entries = vec![("translate", Tag::String(JavaString::from(key)))];
    if !args.is_empty() {
        entries.push(("with", Tag::List(args.iter().map(|a| literal(a)).collect())));
    }
    Tag::compound(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fallback_repeating_its_argument_stays_bounded() {
        // 16 000 references to the argument, nested three deep: 4e12 bytes asked for.
        let pattern = "%1$s".repeat(16_000);
        let mut tag = literal("A");
        for _ in 0..3 {
            tag = Tag::compound(vec![
                ("translate", Tag::String(JavaString::from("x.unknown"))),
                ("fallback", Tag::String(JavaString::from(pattern.as_str()))),
                ("with", Tag::List(vec![tag])),
            ]);
        }
        let started = std::time::Instant::now();
        let text = plain(&tag, &Language::new());
        assert!(text.len() <= MAX_TEXT);
        assert!(
            text.len() > MAX_TEXT - 4,
            "filled up to the cap: {}",
            text.len()
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        // A cut never splits a character.
        let mut out = "x".repeat(MAX_TEXT - 1);
        push_capped(&mut out, "é");
        assert_eq!(out.len(), MAX_TEXT - 1);
        // An ordinary fallback still formats.
        let ok = Tag::compound(vec![
            ("translate", Tag::String(JavaString::from("x.unknown"))),
            ("fallback", Tag::String(JavaString::from("%1$s and %1$s"))),
            ("with", Tag::List(vec![literal("A")])),
        ]);
        assert_eq!(plain(&ok, &Language::new()), "A and A");
    }

    #[test]
    fn formats_like_translatable_contents() {
        let args = vec!["Steve".to_owned(), "hi".to_owned()];
        assert_eq!(format_java("<%s> %s", &args), "<Steve> hi");
        assert_eq!(format_java("%2$s by %1$s", &args), "hi by Steve");
        assert_eq!(format_java("100%% sure", &[]), "100% sure");
    }

    #[test]
    fn unwraps_mixed_list_arguments() {
        let mut lang = Language::new();
        lang.insert(
            "commands.item.entity.set.success.single".into(),
            "Replaced %s slot(s) on %s with %s".into(),
        );
        // `with: [1, {text: "Alex"}, ...]` in NBT: the int comes wrapped.
        let replaced = Tag::compound([
            (
                "translate",
                Tag::String("commands.item.entity.set.success.single".into()),
            ),
            (
                "with",
                Tag::List(vec![
                    Tag::compound([("", Tag::Int(1))]),
                    Tag::compound([("text", Tag::String("Alex".into()))]),
                    Tag::compound([("", Tag::String("[Emerald]".into()))]),
                ]),
            ),
        ]);
        assert_eq!(
            plain(&replaced, &lang),
            "Replaced 1 slot(s) on Alex with [Emerald]"
        );
    }

    #[test]
    fn flattens_components() {
        let mut lang = Language::new();
        lang.insert(
            "multiplayer.player.joined".into(),
            "%s joined the game".into(),
        );
        let joined = Tag::compound([
            ("translate", Tag::String("multiplayer.player.joined".into())),
            (
                "with",
                Tag::List(vec![Tag::compound([("text", Tag::String("Alex".into()))])]),
            ),
            ("color", Tag::String("yellow".into())),
        ]);
        assert_eq!(plain(&joined, &lang), "Alex joined the game");
        let extra = Tag::compound([
            ("text", Tag::String("a".into())),
            (
                "extra",
                Tag::List(vec![
                    literal("b"),
                    Tag::compound([("text", Tag::String("c".into()))]),
                ]),
            ),
        ]);
        assert_eq!(plain(&extra, &lang), "abc");
        assert_eq!(
            plain(&translatable("missing.key", &[]), &lang),
            "missing.key"
        );
        assert_eq!(plain(&literal("plain"), &lang), "plain");
    }
}
