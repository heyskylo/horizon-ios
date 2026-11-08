//! Splits an expression's source into tokens.

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Num(f64),
    Str(String),
    Ident(String),
    /// Operators and punctuation, longest match first (`<=` before `<`).
    Punct(&'static str),
}

const PUNCT: [&str; 29] = [
    "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "(", ")", "{", "}", ",", ";", ".",
    "?", ":", "+", "-", "*", "/", "%", "^", "!", "=", "<", ">",
];

/// The tokens and the byte offset each starts at.
pub(super) fn tokens(src: &str) -> Result<Vec<(Token, usize)>, String> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let token = if c == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let digits = &src[start + 2..i];
            Token::Num(
                u64::from_str_radix(digits, 16).map_err(|e| format!("{e} at {start}"))? as f64,
            )
        } else if c.is_ascii_digit()
            || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                let sign = usize::from(matches!(bytes.get(i + 1), Some(b'+' | b'-')));
                if bytes.get(i + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    i += 1 + sign;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &src[start..i];
            Token::Num(
                text.parse()
                    .map_err(|_| format!("bad number {text:?} at {start}"))?,
            )
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Token::Ident(src[start..i].to_owned())
        } else if c == b'\'' || c == b'"' {
            let end = src[i + 1..]
                .find(c as char)
                .ok_or_else(|| format!("unclosed string at {start}"))?;
            i += end + 2;
            Token::Str(src[start + 1..i - 1].to_owned())
        } else {
            let p = PUNCT
                .iter()
                .find(|p| src[i..].starts_with(**p))
                .ok_or_else(|| {
                    format!(
                        "unexpected {:?} at {start}",
                        src[i..].chars().next().unwrap_or(' ')
                    )
                })?;
            i += p.len();
            Token::Punct(p)
        };
        out.push((token, start));
    }
    Ok(out)
}
