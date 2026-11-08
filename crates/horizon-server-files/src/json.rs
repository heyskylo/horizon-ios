//! The JSON the dedicated server reads and writes through Gson: a tree that
//! keeps object member order and the literal text of numbers (Gson's
//! LazilyParsedNumber), a strict parser (`Strictness.STRICT`), and Gson's
//! compact (`JsonElement.toString`) and pretty, HTML-safe writers.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The number's literal text.
    Number(String),
    String(String),
    Array(Vec<Json>),
    /// Members in insertion order; a repeated name replaces the value in place.
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// JsonElement.getAsString for primitives.
    pub fn as_string(&self) -> Option<String> {
        match self {
            Json::String(s) | Json::Number(s) => Some(s.clone()),
            Json::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
    /// JsonObject.addProperty: a null value is JsonNull, which the writers skip.
    pub fn add(&mut self, key: &str, value: Json) {
        if let Json::Object(m) = self {
            if let Some(slot) = m.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                m.push((key.to_owned(), value));
            }
        }
    }

    /// JsonElement.toString: compact, not HTML-safe, nulls in objects written.
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }
    fn write_compact(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => out.push_str(n),
            Json::String(s) => quote(out, s, false),
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_compact(out);
                }
                out.push(']');
            }
            Json::Object(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    quote(out, k, false);
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
    }

    /// `new GsonBuilder().create().toJson(element)`: compact, HTML-safe
    /// strings, null members skipped.
    pub fn gson(&self) -> String {
        let mut out = String::new();
        self.write_gson(&mut out);
        out
    }
    fn write_gson(&self, out: &mut String) {
        match self {
            Json::String(s) => quote(out, s, true),
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_gson(out);
                }
                out.push(']');
            }
            Json::Object(m) => {
                out.push('{');
                let members = m.iter().filter(|(_, v)| *v != Json::Null);
                for (i, (k, v)) in members.enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    quote(out, k, true);
                    out.push(':');
                    v.write_gson(out);
                }
                out.push('}');
            }
            other => other.write_compact(out),
        }
    }

    /// `new GsonBuilder().setPrettyPrinting().create().toJson(element, writer)`:
    /// two-space indents, HTML-safe strings, null members skipped.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }
    fn write_pretty(&self, out: &mut String, depth: usize) {
        let indent = |out: &mut String, d: usize| {
            out.push('\n');
            for _ in 0..d {
                out.push_str("  ");
            }
        };
        match self {
            Json::Array(items) => {
                out.push('[');
                if items.is_empty() {
                    out.push(']');
                    return;
                }
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    v.write_pretty(out, depth + 1);
                }
                indent(out, depth);
                out.push(']');
            }
            Json::Object(m) => {
                out.push('{');
                let members: Vec<_> = m.iter().filter(|(_, v)| *v != Json::Null).collect();
                if members.is_empty() {
                    out.push('}');
                    return;
                }
                for (i, (k, v)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    quote(out, k, true);
                    out.push_str(": ");
                    v.write_pretty(out, depth + 1);
                }
                indent(out, depth);
                out.push('}');
            }
            Json::String(s) => quote(out, s, true),
            other => other.write_compact(out),
        }
    }
}

/// JsonWriter.string.
fn quote(out: &mut String, s: &str, html_safe: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '<' | '>' | '&' | '=' | '\'' if html_safe => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A strict JSON document: one value, optionally followed only by whitespace
/// when `whole` is set.
pub fn parse(text: &str, whole: bool) -> Result<Json, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = Parser { c: &chars, i: 0 };
    let v = p.value(0)?;
    if whole {
        p.ws();
        if p.i != chars.len() {
            return Err("Did not consume the entire document.".into());
        }
    }
    Ok(v)
}

/// The deepest nesting a document may parse: a file past this is not a list
/// the server reads, and the recursion is bounded (serde's own limit is 128).
const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    c: &'a [char],
    i: usize,
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self
            .c
            .get(self.i)
            .is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
        {
            self.i += 1;
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth >= MAX_DEPTH {
            return Err("Nesting too deep".into());
        }
        self.ws();
        match self.c.get(self.i) {
            None => Err("End of input".into()),
            Some('{') => {
                self.i += 1;
                let mut m: Vec<(String, Json)> = Vec::new();
                self.ws();
                if self.c.get(self.i) == Some(&'}') {
                    self.i += 1;
                    return Ok(Json::Object(m));
                }
                loop {
                    self.ws();
                    if self.c.get(self.i) != Some(&'"') {
                        return Err("Expected name".into());
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.c.get(self.i) != Some(&':') {
                        return Err("Expected ':'".into());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    if let Some(slot) = m.iter_mut().find(|(n, _)| *n == k) {
                        slot.1 = v;
                    } else {
                        m.push((k, v));
                    }
                    self.ws();
                    match self.c.get(self.i) {
                        Some(',') => self.i += 1,
                        Some('}') => {
                            self.i += 1;
                            return Ok(Json::Object(m));
                        }
                        _ => return Err("Unterminated object".into()),
                    }
                }
            }
            Some('[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.c.get(self.i) == Some(&']') {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.c.get(self.i) {
                        Some(',') => self.i += 1,
                        Some(']') => {
                            self.i += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err("Unterminated array".into()),
                    }
                }
            }
            Some('"') => Ok(Json::String(self.string()?)),
            Some(_) => {
                let start = self.i;
                while self
                    .c
                    .get(self.i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.'))
                {
                    self.i += 1;
                }
                let word: String = self.c[start..self.i].iter().collect();
                match word.as_str() {
                    "true" => Ok(Json::Bool(true)),
                    "false" => Ok(Json::Bool(false)),
                    "null" => Ok(Json::Null),
                    w if is_number(w) => Ok(Json::Number(w.to_owned())),
                    _ => Err(format!("Malformed JSON at {start}")),
                }
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        let mut units: Vec<u16> = Vec::new();
        loop {
            let Some(&c) = self.c.get(self.i) else {
                return Err("Unterminated string".into());
            };
            self.i += 1;
            if c != '\\' && !units.is_empty() {
                out.push_str(&String::from_utf16_lossy(&units));
                units.clear();
            }
            match c {
                '"' => {
                    out.push_str(&String::from_utf16_lossy(&units));
                    return Ok(out);
                }
                '\\' => {
                    let Some(&e) = self.c.get(self.i) else {
                        return Err("Unterminated escape".into());
                    };
                    self.i += 1;
                    if e == 'u' {
                        let hex: String = self
                            .c
                            .get(self.i..self.i + 4)
                            .ok_or("bad escape")?
                            .iter()
                            .collect();
                        let v = u16::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
                        self.i += 4;
                        units.push(v);
                        continue;
                    }
                    if !units.is_empty() {
                        out.push_str(&String::from_utf16_lossy(&units));
                        units.clear();
                    }
                    out.push(match e {
                        't' => '\t',
                        'b' => '\u{8}',
                        'n' => '\n',
                        'r' => '\r',
                        'f' => '\u{c}',
                        '"' | '\\' | '/' => e,
                        _ => return Err("Invalid escape sequence".into()),
                    });
                }
                c if (c as u32) < 0x20 => return Err("Unescaped control character".into()),
                c => out.push(c),
            }
        }
    }
}

/// JSON number syntax (JsonReader.peekNumber in strict mode).
fn is_number(w: &str) -> bool {
    let b = w.as_bytes();
    let mut i = 0;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    match b.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return false,
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    i == b.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips() {
        let v = parse(r#" { "a" : 1.50 , "b":[true,null,"x<"] , "a": 2 } "#, true).unwrap();
        assert_eq!(v.compact(), r#"{"a":2,"b":[true,null,"x<"]}"#);
        let mut o = Json::Object(vec![]);
        o.add("name", Json::String("a=b".into()));
        o.add("reason", Json::Null);
        assert_eq!(
            Json::Array(vec![o]).pretty(),
            "[\n  {\n    \"name\": \"a\\u003db\"\n  }\n]"
        );
        assert_eq!(Json::Array(vec![]).pretty(), "[]");
        assert!(parse("{a:1}", true).is_err());
    }

    /// A document nested past the cap is refused, not recursed into forever:
    /// files and remote responses the server reads can be any nesting.
    #[test]
    fn nesting_is_bounded() {
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert!(parse(&ok, true).is_ok());
        assert!(parse(&deep, true).is_err());
        assert!(parse(&deep, false).is_err());
    }
}
