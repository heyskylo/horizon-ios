//! `StringTagVisitor` (compact SNBT, `Tag.toString`) and Java's number
//! formatting, below the text-component layer so that the protocol can hash
//! a tag's string form.
use crate::nbt::Tag;

/// `Double.toString`.
pub fn java_double(v: f64) -> String {
    java_float_text(v, &format!("{v}"), &format!("{v:e}"))
}
/// `Float.toString`.
pub fn java_float(v: f32) -> String {
    java_float_text(f64::from(v), &format!("{v}"), &format!("{v:e}"))
}
fn java_float_text(v: f64, plain: &str, sci: &str) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = v.abs();
    if (1.0e-3..1.0e7).contains(&a) {
        if plain.contains('.') {
            plain.to_owned()
        } else {
            format!("{plain}.0")
        }
    } else {
        let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci, "0"));
        if !mantissa.contains('.') {
            // Java also considers two-digit decimals when one digit round-trips,
            // choosing the closest value. Padding Rust's choice loses that step
            // for tiny subnormals (Float.MIN_VALUE is 1.4E-45, not 1.0E-45).
            return format!("{v:.1e}").replace('e', "E");
        }
        format!("{mantissa}E{exponent}")
    }
}

/// `SnbtGrammar.escapeControlCharacters`.
fn escape_control(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{8}' => "b",
        '\u{c}' => "f",
        '\n' => "n",
        '\r' => "r",
        '\t' => "t",
        _ => return None,
    })
}
/// `StringTag.quoteAndEscape`.
pub fn quote_and_escape(input: &str) -> String {
    let mut out = String::from(" ");
    let mut quote = '\0';
    for c in input.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if c != '"' && c != '\'' {
            match escape_control(c) {
                Some(e) => {
                    out.push('\\');
                    out.push_str(e);
                }
                None => out.push(c),
            }
        } else {
            if quote == '\0' {
                quote = if c == '"' { '\'' } else { '"' };
            }
            if quote == c {
                out.push('\\');
            }
            out.push(c);
        }
    }
    if quote == '\0' {
        quote = '"';
    }
    out.replace_range(0..1, &quote.to_string());
    out.push(quote);
    out
}
fn unquoted_key(key: &str) -> bool {
    if key.eq_ignore_ascii_case("true") || key.eq_ignore_ascii_case("false") {
        return false;
    }
    let mut chars = key.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '.' || c == '_');
    // `[A-Za-z._]+[A-Za-z0-9._+-]*`
    first_ok && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}
pub fn string_of(t: &crate::nbt::JavaString) -> String {
    t.to_string_lossy()
}

/// `Tag.toString` (StringTagVisitor).
pub fn to_snbt(tag: &Tag) -> String {
    let mut out = String::new();
    visit(tag, &mut out);
    out
}
fn visit(tag: &Tag, out: &mut String) {
    match tag {
        Tag::End => out.push_str("END"),
        Tag::Byte(v) => out.push_str(&format!("{v}b")),
        Tag::Short(v) => out.push_str(&format!("{v}s")),
        Tag::Int(v) => out.push_str(&v.to_string()),
        Tag::Long(v) => out.push_str(&format!("{v}L")),
        Tag::Float(v) => {
            out.push_str(&java_float(f32::from_bits(*v)));
            out.push('f');
        }
        Tag::Double(v) => {
            out.push_str(&java_double(f64::from_bits(*v)));
            out.push('d');
        }
        Tag::String(s) => out.push_str(&quote_and_escape(&string_of(s))),
        Tag::ByteArray(a) => {
            out.push_str("[B;");
            out.push_str(
                &a.iter()
                    .map(|v| format!("{}B", *v as i8))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push(']');
        }
        Tag::IntArray(a) => {
            out.push_str("[I;");
            out.push_str(&a.iter().map(i32::to_string).collect::<Vec<_>>().join(","));
            out.push(']');
        }
        Tag::LongArray(a) => {
            out.push_str("[L;");
            out.push_str(
                &a.iter()
                    .map(|v| format!("{v}L"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push(']');
        }
        Tag::List(l) => {
            out.push('[');
            for (i, t) in l.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                visit(t, out);
            }
            out.push(']');
        }
        Tag::Compound(c) => {
            out.push('{');
            // JavaString keys already follow StringTagVisitor's UTF-16 order.
            // Sorting converted Rust strings would put supplementary keys last.
            for (i, (k, v)) in c.iter().enumerate() {
                let k = string_of(k);
                if i > 0 {
                    out.push(',');
                }
                if unquoted_key(&k) {
                    out.push_str(&k);
                } else {
                    out.push_str(&quote_and_escape(&k));
                }
                out.push(':');
                visit(v, out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scientific_text_selects_the_closest_two_digit_decimal() {
        for (bits, expected) in [
            (1, "1.4E-45"),
            (2, "2.8E-45"),
            (7, "9.8E-45"),
            (71, "9.9E-44"),
            (100, "1.4E-43"),
        ] {
            for (sign, prefix) in [(0, ""), (1 << 31, "-")] {
                let bits = bits | sign;
                assert_eq!(
                    java_float(f32::from_bits(bits)),
                    format!("{prefix}{expected}")
                );
                assert_eq!(to_snbt(&Tag::Float(bits)), format!("{prefix}{expected}f"));
            }
        }
        for (bits, expected) in [
            (1, "4.9E-324"),
            (2, "9.9E-324"),
            (12, "5.9E-323"),
            (100, "4.94E-322"),
        ] {
            for (sign, prefix) in [(0, ""), (1 << 63, "-")] {
                let bits = bits | sign;
                assert_eq!(
                    java_double(f64::from_bits(bits)),
                    format!("{prefix}{expected}")
                );
                assert_eq!(to_snbt(&Tag::Double(bits)), format!("{prefix}{expected}d"));
            }
        }
    }
}
