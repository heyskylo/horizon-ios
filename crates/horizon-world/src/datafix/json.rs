//! The JSON that DataFixers read and write: Gson's `JsonReader` in strict and
//! lenient mode, `GsonHelper.toStableString`, and `JsonOps.convertTo` into NBT.
//! Strings stay UTF-16 like Java's; numbers keep their literal like Gson's
//! `LazilyParsedNumber`.
use crate::nbt::{Compound, JavaString, Tag};

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// The literal as written (`LazilyParsedNumber`) or `Number.toString`.
    Num(String),
    Str(Vec<u16>),
    Arr(Vec<Json>),
    /// Insertion order, a repeated key replacing the value in place.
    Obj(Vec<(Vec<u16>, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        let key: Vec<u16> = key.encode_utf16().collect();
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn insert(entries: &mut Vec<(Vec<u16>, Json)>, key: Vec<u16>, value: Json) {
        if let Some(e) = entries.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            entries.push((key, value));
        }
    }
    pub fn object(entries: Vec<(&str, Json)>) -> Json {
        let mut out = Vec::new();
        for (k, v) in entries {
            Json::insert(&mut out, k.encode_utf16().collect(), v);
        }
        Json::Obj(out)
    }
    pub fn string(s: &str) -> Json {
        Json::Str(s.encode_utf16().collect())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Strict,
    Lenient,
    GsonPrefix,
}

struct Reader<'a> {
    s: &'a [u16],
    pos: usize,
    mode: Mode,
}

type R<T> = Result<T, ()>;

const fn u(c: char) -> u16 {
    c as u16
}

impl Reader<'_> {
    fn lenient(&self) -> bool {
        self.mode == Mode::Lenient
    }
    fn peek_char(&self) -> Option<u16> {
        self.s.get(self.pos).copied()
    }
    /// `JsonReader.nextNonWhitespace`, with comments when lenient.
    fn skip_ws(&mut self) -> R<Option<u16>> {
        loop {
            let Some(c) = self.peek_char() else {
                return Ok(None);
            };
            match c {
                0x20 | 0x0A | 0x09 | 0x0D => self.pos += 1,
                0x2F if self.s.get(self.pos + 1) == Some(&u('*'))
                    || self.s.get(self.pos + 1) == Some(&u('/')) =>
                {
                    if !self.lenient() {
                        return Err(());
                    }
                    if self.s[self.pos + 1] == u('*') {
                        let start = self.pos + 2;
                        let end = (start..self.s.len().saturating_sub(1))
                            .find(|&i| self.s[i] == u('*') && self.s[i + 1] == u('/'))
                            .ok_or(())?;
                        self.pos = end + 2;
                    } else {
                        self.skip_line();
                    }
                }
                0x23 => {
                    if !self.lenient() {
                        return Err(());
                    }
                    self.skip_line();
                }
                _ => return Ok(Some(c)),
            }
        }
    }
    fn skip_line(&mut self) {
        while let Some(c) = self.peek_char() {
            self.pos += 1;
            if c == 0x0A || c == 0x0D {
                break;
            }
        }
    }
    fn value(&mut self, depth: usize) -> R<Json> {
        if depth > 255 {
            return Err(());
        }
        let c = self.skip_ws()?.ok_or(())?;
        match c {
            0x7B => self.object(depth),
            0x5B => self.array(depth),
            0x22 => {
                self.pos += 1;
                Ok(Json::Str(self.quoted(0x22)?))
            }
            0x27 => {
                if !self.lenient() {
                    return Err(());
                }
                self.pos += 1;
                Ok(Json::Str(self.quoted(0x27)?))
            }
            _ => self.literal(),
        }
    }
    fn object(&mut self, depth: usize) -> R<Json> {
        self.pos += 1;
        let mut entries = Vec::new();
        if self.skip_ws()? == Some(u('}')) {
            self.pos += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            let c = self.skip_ws()?.ok_or(())?;
            let name = match c {
                0x22 => {
                    self.pos += 1;
                    self.quoted(0x22)?
                }
                0x27 if self.lenient() => {
                    self.pos += 1;
                    self.quoted(0x27)?
                }
                _ if self.lenient() => {
                    let lit = self.unquoted();
                    if lit.is_empty() {
                        return Err(());
                    }
                    lit
                }
                _ => return Err(()),
            };
            match self.skip_ws()?.ok_or(())? {
                0x3A => self.pos += 1,
                0x3D if self.lenient() => {
                    self.pos += 1;
                    if self.peek_char() == Some(u('>')) {
                        self.pos += 1;
                    }
                }
                _ => return Err(()),
            }
            let value = self.value(depth + 1)?;
            Json::insert(&mut entries, name, value);
            match self.skip_ws()?.ok_or(())? {
                0x7D => {
                    self.pos += 1;
                    return Ok(Json::Obj(entries));
                }
                0x2C => self.pos += 1,
                0x3B if self.lenient() => self.pos += 1,
                _ => return Err(()),
            }
        }
    }
    fn array(&mut self, depth: usize) -> R<Json> {
        self.pos += 1;
        let mut out = Vec::new();
        if self.skip_ws()? == Some(u(']')) {
            self.pos += 1;
            return Ok(Json::Arr(out));
        }
        loop {
            let c = self.skip_ws()?.ok_or(())?;
            // Lenient: an empty element between separators is a null.
            if self.lenient() && (c == u(',') || c == u(';')) {
                out.push(Json::Null);
                self.pos += 1;
                continue;
            }
            if self.lenient() && c == u(']') {
                out.push(Json::Null);
                self.pos += 1;
                return Ok(Json::Arr(out));
            }
            out.push(self.value(depth + 1)?);
            match self.skip_ws()?.ok_or(())? {
                0x5D => {
                    self.pos += 1;
                    return Ok(Json::Arr(out));
                }
                0x2C => self.pos += 1,
                0x3B if self.lenient() => self.pos += 1,
                _ => return Err(()),
            }
        }
    }
    fn quoted(&mut self, quote: u16) -> R<Vec<u16>> {
        let mut out = Vec::new();
        loop {
            let c = self.peek_char().ok_or(())?;
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c == u('\\') {
                let e = self.peek_char().ok_or(())?;
                self.pos += 1;
                match e {
                    0x75 => {
                        let hex = self.s.get(self.pos..self.pos + 4).ok_or(())?;
                        let text = String::from_utf16(hex).map_err(|_| ())?;
                        out.push(u16::from_str_radix(&text, 16).map_err(|_| ())?);
                        self.pos += 4;
                    }
                    0x74 => out.push(u('\t')),
                    0x62 => out.push(0x08),
                    0x6E => out.push(u('\n')),
                    0x72 => out.push(u('\r')),
                    0x66 => out.push(0x0C),
                    0x0A | 0x22 | 0x5C | 0x2F => out.push(e),
                    0x27 => out.push(e),
                    _ if self.lenient() => out.push(e),
                    _ => return Err(()),
                }
            } else {
                if !self.lenient() && c < 0x20 {
                    return Err(());
                }
                out.push(c);
            }
        }
    }
    fn unquoted(&mut self) -> Vec<u16> {
        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if matches!(
                c,
                0x2F | 0x5C
                    | 0x3B
                    | 0x23
                    | 0x3D
                    | 0x7B
                    | 0x7D
                    | 0x5B
                    | 0x5D
                    | 0x3A
                    | 0x2C
                    | 0x20
                    | 0x09
                    | 0x0C
                    | 0x0D
                    | 0x0A
            ) {
                break;
            }
            self.pos += 1;
        }
        self.s[start..self.pos].to_vec()
    }
    /// A keyword, a number, or (lenient) an unquoted string.
    fn literal(&mut self) -> R<Json> {
        let lit = self.unquoted();
        if lit.is_empty() {
            return Err(());
        }
        // Gson's strict isLiteral check rejects these delimiters while scanning a primitive.
        // After a quoted string or container, GsonHelper does not inspect trailing input at all.
        if self.mode == Mode::GsonPrefix
            && matches!(self.peek_char(), Some(0x2F | 0x5C | 0x3B | 0x23 | 0x3D))
        {
            return Err(());
        }
        let text = String::from_utf16_lossy(&lit);
        for (word, value) in [
            ("true", Json::Bool(true)),
            ("false", Json::Bool(false)),
            ("null", Json::Null),
        ] {
            if text.len() == word.len()
                && text.chars().zip(word.chars()).all(|(a, b)| {
                    a == b
                        || (self.lenient() || self.mode == Mode::GsonPrefix)
                            && a == b.to_ascii_uppercase()
                })
            {
                return Ok(value);
            }
        }
        if is_json_number(&text) {
            return Ok(Json::Num(text));
        }
        if self.lenient() {
            return Ok(Json::Str(lit));
        }
        Err(())
    }
}

/// `JsonReader.peekNumber`.
fn is_json_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    match b.get(i) {
        Some(b'0') => {
            i += 1;
            if b.get(i).is_some_and(u8::is_ascii_digit) {
                return false;
            }
        }
        Some(c) if c.is_ascii_digit() => {
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

fn parse_mode(s: &[u16], mode: Mode) -> Option<Json> {
    let mut r = Reader { s, pos: 0, mode };
    let value = r.value(0).ok()?;
    // JsonParser.parseReader: the whole document must be consumed.
    if r.skip_ws().ok()?.is_some() {
        if mode == Mode::Lenient && value == Json::Null {
            return Some(value);
        }
        return None;
    }
    Some(value)
}

/// `LenientJsonParser.parse` (Gson `JsonParser.parseString`).
pub fn parse_lenient(s: &[u16]) -> Option<Json> {
    parse_mode(s, Mode::Lenient)
}
/// `StrictJsonParser.parse`.
pub fn parse_strict(s: &[u16]) -> Option<Json> {
    parse_mode(s, Mode::Strict)
}

/// `GsonHelper.fromJson`: read one value without requiring the end of the document.
/// Container and string syntax is strict; Gson's legacy case-insensitive keywords remain accepted.
pub fn parse_prefix_strict(s: &[u16]) -> Option<Json> {
    Reader {
        s,
        pos: 0,
        mode: Mode::GsonPrefix,
    }
    .value(0)
    .ok()
}

/// `GsonHelper.toStableString`: keys in natural (UTF-16) order, Gson escaping.
pub fn to_stable_string(json: &Json) -> Vec<u16> {
    let mut out = Vec::new();
    write(json, &mut out);
    out
}
fn write(json: &Json, out: &mut Vec<u16>) {
    let push = |out: &mut Vec<u16>, s: &str| out.extend(s.encode_utf16());
    match json {
        Json::Null => push(out, "null"),
        Json::Bool(b) => push(out, if *b { "true" } else { "false" }),
        Json::Num(n) => push(out, n),
        Json::Str(s) => write_string(s, out),
        Json::Arr(items) => {
            out.push(u('['));
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(u(','));
                }
                write(item, out);
            }
            out.push(u(']'));
        }
        Json::Obj(entries) => {
            let mut sorted: Vec<&(Vec<u16>, Json)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push(u('{'));
            for (i, (k, v)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(u(','));
                }
                write_string(k, out);
                out.push(u(':'));
                write(v, out);
            }
            out.push(u('}'));
        }
    }
}
/// `JsonWriter.string` without HTML escaping.
fn write_string(s: &[u16], out: &mut Vec<u16>) {
    out.push(u('"'));
    for &c in s {
        match c {
            0x22 => out.extend("\\\"".encode_utf16()),
            0x5C => out.extend("\\\\".encode_utf16()),
            0x09 => out.extend("\\t".encode_utf16()),
            0x08 => out.extend("\\b".encode_utf16()),
            0x0A => out.extend("\\n".encode_utf16()),
            0x0D => out.extend("\\r".encode_utf16()),
            0x0C => out.extend("\\f".encode_utf16()),
            c if c < 0x20 || c == 0x2028 || c == 0x2029 => {
                out.extend(format!("\\u{c:04x}").encode_utf16());
            }
            c => out.push(c),
        }
    }
    out.push(u('"'));
}

/// `JsonOps.convertTo(NbtOps)`. Nulls give `None` (an end tag in Java).
pub fn to_nbt(json: &Json) -> Option<Tag> {
    Some(match json {
        Json::Null => return None,
        Json::Bool(b) => Tag::Byte(i8::from(*b)),
        Json::Str(s) => Tag::String(JavaString(s.clone())),
        Json::Num(n) => number_to_nbt(n),
        Json::Arr(items) => Tag::List(items.iter().filter_map(to_nbt).collect()),
        Json::Obj(entries) => {
            let mut c = Compound::new();
            for (k, v) in entries {
                if let Some(v) = to_nbt(v) {
                    c.insert(JavaString(k.clone()), v);
                }
            }
            Tag::Compound(c)
        }
    })
}

/// `JsonOps.convertTo` for a number: the smallest integer tag that holds an
/// exact integral value, else float when exact, else double.
fn number_to_nbt(literal: &str) -> Tag {
    if let Some(l) = exact_long(literal) {
        if let Ok(b) = i8::try_from(l) {
            return Tag::Byte(b);
        }
        if let Ok(s) = i16::try_from(l) {
            return Tag::Short(s);
        }
        if let Ok(i) = i32::try_from(l) {
            return Tag::Int(i);
        }
        return Tag::Long(l);
    }
    let d: f64 = literal.parse().unwrap_or(0.0);
    let f = d as f32;
    if f64::from(f) == d {
        Tag::Float(f.to_bits())
    } else {
        Tag::Double(d.to_bits())
    }
}

/// `BigDecimal.longValueExact` of a JSON number literal.
fn exact_long(literal: &str) -> Option<i64> {
    let (mantissa, exponent) = match literal.find(['e', 'E']) {
        Some(i) => (&literal[..i], literal[i + 1..].parse::<i64>().ok()?),
        None => (literal, 0),
    };
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let (int_part, frac) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    let mut digits: String = format!("{int_part}{frac}");
    let mut scale = frac.len() as i64 - exponent;
    // Trailing zeros of the unscaled value do not change the value.
    while scale > 0 && digits.ends_with('0') {
        digits.pop();
        scale -= 1;
    }
    if scale > 0 {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    if digits.len() as i64 - scale > 19 {
        return None;
    }
    let mut value: i128 = digits.parse().ok()?;
    for _ in 0..-scale {
        value = value.checked_mul(10)?;
    }
    let value = if negative { -value } else { value };
    i64::try_from(value).ok()
}

/// `NbtOps.convertTo(JsonOps)`.
pub fn from_nbt(tag: &Tag) -> Json {
    match tag {
        Tag::End => Json::Null,
        Tag::Byte(v) => Json::Num(v.to_string()),
        Tag::Short(v) => Json::Num(v.to_string()),
        Tag::Int(v) => Json::Num(v.to_string()),
        Tag::Long(v) => Json::Num(v.to_string()),
        Tag::Float(v) => Json::Num(java_float_string(f32::from_bits(*v))),
        Tag::Double(v) => Json::Num(java_double_string(f64::from_bits(*v))),
        Tag::ByteArray(v) => Json::Arr(
            v.iter()
                .map(|b| Json::Num((*b as i8).to_string()))
                .collect(),
        ),
        Tag::IntArray(v) => Json::Arr(v.iter().map(|i| Json::Num(i.to_string())).collect()),
        Tag::LongArray(v) => Json::Arr(v.iter().map(|l| Json::Num(l.to_string())).collect()),
        Tag::String(s) => Json::Str(s.0.clone()),
        Tag::List(items) => Json::Arr(items.iter().map(from_nbt).collect()),
        Tag::Compound(c) => Json::Obj(c.iter().map(|(k, v)| (k.0.clone(), from_nbt(v))).collect()),
    }
}

/// Java `Double.toString`.
pub fn java_double_string(d: f64) -> String {
    java_fp_string(d, format!("{d:e}"))
}
/// Java `Float.toString`.
pub fn java_float_string(f: f32) -> String {
    java_fp_string(f64::from(f), format!("{f:e}"))
}
fn java_fp_string(d: f64, sci: String) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // `sci` is Rust's shortest round-trip form: d.ddde<exp>.
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        if exp >= 0 {
            let e = exp as usize;
            let (int, frac) = if digits.len() > e + 1 {
                (digits[..=e].to_string(), digits[e + 1..].to_string())
            } else {
                (format!("{digits:0<width$}", width = e + 1), String::new())
            };
            let frac = if frac.is_empty() { "0".into() } else { frac };
            format!("{sign}{int}.{frac}")
        } else {
            let zeros = "0".repeat((-exp - 1) as usize);
            format!("{sign}0.{zeros}{digits}")
        }
    } else {
        if digits.len() == 1 {
            // Select the nearest two-digit decimal, rather than padding the
            // one-digit round-trip form; Java can choose a different exponent.
            return format!("{d:.1e}").replace('e', "E");
        }
        let (first, rest) = digits.split_at(1);
        format!("{sign}{first}.{rest}E{exp}")
    }
}

/// A string value as a JSON text component (`{"text": s}`), stable string.
pub fn text_component_json(text: &[u16]) -> Vec<u16> {
    to_stable_string(&Json::Obj(vec![(
        "text".encode_utf16().collect(),
        Json::Str(text.to_vec()),
    )]))
}
pub fn translatable_component_json(key: &str) -> Vec<u16> {
    to_stable_string(&Json::object(vec![("translate", Json::string(key))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
    #[test]
    fn gson_helper_reads_one_value_but_retains_strict_token_boundaries() {
        // Independent 1.20.2 FriendlyByteBuf.readComponent/GsonHelper oracle results.
        for input in [r#""x" junk"#, r#""x"/*ignored suffix*/"#] {
            assert_eq!(parse_prefix_strict(&w(input)), Some(Json::string("x")));
            assert!(parse_strict(&w(input)).is_none());
        }
        assert_eq!(parse_prefix_strict(&w("1 2")), Some(Json::Num("1".into())));
        for input in ["true:", "true]", "TRUE"] {
            assert_eq!(parse_prefix_strict(&w(input)), Some(Json::Bool(true)));
        }
        assert_eq!(parse_prefix_strict(&w("NULL")), Some(Json::Null));
        assert_eq!(
            parse_prefix_strict(&w(r#"{"text":"x"}junk"#)),
            Some(Json::object(vec![("text", Json::string("x"))]))
        );
        for input in [
            "truejunk",
            "falsex",
            "nullx",
            "12junk",
            "1.2x",
            "true/comment",
            "1#x",
            "{text:x}",
            "{'text':'x'}",
            r#"{"text":"x",}"#,
            "[1,]",
            "/*comment*/1",
        ] {
            assert!(parse_prefix_strict(&w(input)).is_none(), "{input}");
        }
    }
    #[test]
    fn numbers_become_the_smallest_tag() {
        let tag = |s: &str| to_nbt(&parse_strict(&w(s)).unwrap()).unwrap();
        assert_eq!(tag("1"), Tag::Byte(1));
        assert_eq!(tag("1.0"), Tag::Byte(1));
        assert_eq!(tag("1e2"), Tag::Byte(100));
        assert_eq!(tag("300"), Tag::Short(300));
        assert_eq!(tag("1.5"), Tag::Float(1.5f32.to_bits()));
        assert_eq!(tag("0.1"), Tag::Double(0.1f64.to_bits()));
        assert_eq!(tag("3000000000"), Tag::Long(3_000_000_000));
    }
    #[test]
    fn lenient_reads_what_strict_refuses() {
        assert!(parse_strict(&w("hello")).is_none());
        assert_eq!(parse_lenient(&w("hello")), Some(Json::string("hello")));
        assert!(parse_lenient(&w("hello world")).is_none());
        assert_eq!(
            parse_lenient(&w("{a:'b'}")),
            Some(Json::object(vec![("a", Json::string("b"))]))
        );
    }
    #[test]
    fn stable_strings_sort_keys() {
        let j = parse_strict(&w(r#"{"text":"a","color":"red"}"#)).unwrap();
        assert_eq!(
            String::from_utf16(&to_stable_string(&j)).unwrap(),
            r#"{"color":"red","text":"a"}"#
        );
    }
    #[test]
    fn java_floating_point_strings() {
        assert_eq!(java_float_string(f32::from_bits(1)), "1.4E-45");
        assert_eq!(java_float_string(f32::from_bits(0x8000_0007)), "-9.8E-45");
        assert_eq!(java_double_string(f64::from_bits(1)), "4.9E-324");
        assert_eq!(
            java_double_string(f64::from_bits(0x8000_0000_0000_0002)),
            "-9.9E-324"
        );
        assert_eq!(java_double_string(1.0), "1.0");
        assert_eq!(java_double_string(0.001), "0.001");
        assert_eq!(java_double_string(1e7), "1.0E7");
        assert_eq!(java_double_string(123456.789), "123456.789");
        assert_eq!(java_double_string(1e-4), "1.0E-4");
        assert_eq!(java_float_string(0.1), "0.1");
        assert_eq!(java_float_string(1.5e10), "1.5E10");
    }
}
