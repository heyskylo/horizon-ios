//! SnbtGrammar and TagParser: vanilla's packrat SNBT grammar, with its
//! error messages and completion suggestions, producing NBT tags.
use super::packrat::{Grammar, Reason, State, Sugg, Term, Value, alt, ch, chs, look, opt, seq};
use super::{CmdResult, CommandError, Reader, SuggestionsBuilder};
use horizon_world::nbt::{Compound, JavaString, Tag};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeSuffix {
    Float,
    Double,
    Byte,
    Short,
    Int,
    Long,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Binary,
    Decimal,
    Hex,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayPrefix {
    Byte,
    Int,
    Long,
}
#[derive(Clone, Debug)]
pub struct IntLit {
    minus: bool,
    base: Base,
    digits: String,
    /// Some(true) signed, Some(false) unsigned.
    signed: Option<bool>,
    ty: Option<TypeSuffix>,
}

/// The values SnbtGrammar's rules produce.
#[derive(Clone, Debug)]
pub enum SV {
    Tag(Tag),
    Str(String),
    /// A sign: true for minus.
    Sign(bool),
    Suffix(Option<bool>, Option<TypeSuffix>),
    Type(TypeSuffix),
    Int(IntLit),
    /// float exponent: (minus, digits)
    Exp(bool, String),
    Prefix(ArrayPrefix),
    List(Vec<SV>),
    Entry(String, Tag),
}
impl Value for SV {
    fn list(items: Vec<Self>) -> Self {
        SV::List(items)
    }
}
fn delayed(key: &'static str) -> Reason {
    Reason::Delayed(key, Vec::new())
}

fn can_start_number(c: char) -> bool {
    matches!(c, '+' | '-' | '.' | '0'..='9')
}

/// NumberRunParseRule.
fn number_run(
    state: &mut State<SV>,
    accept: fn(char) -> bool,
    no_value: &'static str,
) -> Option<SV> {
    let mut r = state.reader();
    r.skip_whitespace();
    state.set_reader(&r);
    let start = state.cursor;
    let rest = &state.input[start..];
    let len: usize = rest
        .chars()
        .take_while(|c| accept(*c))
        .map(char::len_utf8)
        .sum();
    if len == 0 {
        state.store_at_mark(delayed(no_value));
        return None;
    }
    let s = &rest[..len];
    if s.starts_with('_') || s.ends_with('_') {
        state.store_at_mark(delayed("snbt.parser.underscore_not_allowed"));
        return None;
    }
    state.cursor = start + len;
    Some(SV::Str(s.to_owned()))
}
/// GreedyPredicateParseRule (no whitespace skipping).
fn greedy(
    state: &mut State<SV>,
    accept: fn(char) -> bool,
    min: usize,
    max: usize,
    error: Reason,
) -> Option<SV> {
    let start = state.cursor;
    let rest = &state.input[start..];
    let mut len = 0;
    let mut count = 0;
    for c in rest.chars() {
        if !accept(c) || count >= max {
            break;
        }
        len += c.len_utf8();
        count += c.len_utf16();
    }
    if count < min {
        state.store_at_mark(error);
        return None;
    }
    state.cursor = start + len;
    Some(SV::Str(rest[..len].to_owned()))
}

fn number_error(message: String) -> Reason {
    Reason::Delayed("snbt.parser.number_parse_failure", vec![message])
}
fn for_input(s: &str, radix: u32) -> String {
    if radix == 10 {
        format!("For input string: \"{s}\"")
    } else {
        format!("For input string: \"{s}\" under radix {radix}")
    }
}
/// Long.parseLong(s, radix) with its NumberFormatException messages.
fn parse_long(s: &str, radix: u32) -> Result<i64, String> {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits.is_empty() {
        return Err(for_input(s, radix));
    }
    i64::from_str_radix(s.strip_prefix('+').unwrap_or(s), radix).map_err(|_| for_input(s, radix))
}
fn parse_int(s: &str, radix: u32) -> Result<i32, String> {
    let v = parse_long(s, radix).map_err(|_| for_input(s, radix))?;
    i32::try_from(v).map_err(|_| for_input(s, radix))
}
fn parse_small(s: &str, radix: u32, lo: i32, hi: i32) -> Result<i32, String> {
    let v = parse_int(s, radix)?;
    if v < lo || v > hi {
        return Err(format!("Value out of range. Value:\"{s}\" Radix:{radix}"));
    }
    Ok(v)
}
fn parse_unsigned_int(s: &str, radix: u32) -> Result<i32, String> {
    if s.starts_with('-') {
        return Err(format!(
            "Illegal leading minus sign on unsigned string {s}."
        ));
    }
    let len = s.chars().count();
    if len <= 5 || (radix == 10 && len <= 9) {
        return parse_int(s, radix);
    }
    let v = parse_long(s, radix)?;
    if (v as u64) & 0xffff_ffff_0000_0000 == 0 {
        Ok(v as u32 as i32)
    } else {
        Err(format!("String value {s} exceeds range of unsigned int."))
    }
}
fn parse_unsigned_long(s: &str, radix: u32) -> Result<i64, String> {
    if s.starts_with('-') {
        return Err(format!(
            "Illegal leading minus sign on unsigned string {s}."
        ));
    }
    let digits = s.strip_prefix('+').unwrap_or(s);
    match u64::from_str_radix(digits, radix) {
        Ok(v) => Ok(v as i64),
        Err(_) => {
            if digits.chars().all(|c| c.is_digit(radix)) && !digits.is_empty() {
                Err(format!("String value {s} exceeds range of unsigned long."))
            } else {
                Err(for_input(s, radix))
            }
        }
    }
}

impl IntLit {
    fn signed_or_default(&self) -> bool {
        self.signed.unwrap_or(self.base == Base::Decimal)
    }
    fn cleaned(&self) -> String {
        let mut out = String::new();
        if self.minus {
            out.push('-');
        }
        out.extend(self.digits.chars().filter(|c| *c != '_'));
        out
    }
    /// IntegerLiteral.create with the given type.
    fn create(&self, ty: TypeSuffix, state: &mut State<SV>) -> Option<Tag> {
        let signed = self.signed_or_default();
        if !signed && self.minus {
            state.store_at_mark(delayed("snbt.parser.expected_non_negative_number"));
            return None;
        }
        let s = self.cleaned();
        let radix = match self.base {
            Base::Binary => 2,
            Base::Decimal => 10,
            Base::Hex => 16,
        };
        let r = if signed {
            match ty {
                TypeSuffix::Byte => parse_small(&s, radix, -128, 127).map(|v| Tag::Byte(v as i8)),
                TypeSuffix::Short => {
                    parse_small(&s, radix, -32768, 32767).map(|v| Tag::Short(v as i16))
                }
                TypeSuffix::Int => parse_int(&s, radix).map(Tag::Int),
                TypeSuffix::Long => parse_long(&s, radix).map(Tag::Long),
                _ => {
                    state.store_at_mark(delayed("snbt.parser.expected_integer_type"));
                    return None;
                }
            }
        } else {
            match ty {
                TypeSuffix::Byte => parse_int(&s, radix).and_then(|v| {
                    if v >> 8 == 0 {
                        Ok(Tag::Byte(v as u8 as i8))
                    } else {
                        Err(format!("out of range: {v}"))
                    }
                }),
                TypeSuffix::Short => parse_int(&s, radix).and_then(|v| {
                    if v >> 16 == 0 {
                        Ok(Tag::Short(v as u16 as i16))
                    } else {
                        Err(format!("out of range: {v}"))
                    }
                }),
                TypeSuffix::Int => parse_unsigned_int(&s, radix).map(Tag::Int),
                TypeSuffix::Long => parse_unsigned_long(&s, radix).map(Tag::Long),
                _ => {
                    state.store_at_mark(delayed("snbt.parser.expected_integer_type"));
                    return None;
                }
            }
        };
        match r {
            Ok(t) => Some(t),
            Err(m) => {
                state.store_at_mark(number_error(m));
                None
            }
        }
    }
}

fn float_tag(text: &str, ty: Option<TypeSuffix>, state: &mut State<SV>) -> Option<Tag> {
    match ty {
        Some(TypeSuffix::Float) => {
            let v: f32 = text.parse().unwrap_or(f32::NAN);
            if !v.is_finite() {
                state.store_at_mark(delayed("snbt.parser.infinity_not_allowed"));
                return None;
            }
            Some(Tag::Float(v.to_bits()))
        }
        None | Some(TypeSuffix::Double) => {
            let v: f64 = text.parse().unwrap_or(f64::NAN);
            if !v.is_finite() {
                state.store_at_mark(delayed("snbt.parser.infinity_not_allowed"));
                return None;
            }
            Some(Tag::Double(v.to_bits()))
        }
        _ => {
            state.store_at_mark(delayed("snbt.parser.expected_float_type"));
            None
        }
    }
}

fn get_str(state: &State<SV>, atom: &'static str) -> Option<String> {
    match state.scope.get(atom) {
        Some(SV::Str(s)) => Some(s),
        _ => None,
    }
}
fn get_tag(state: &State<SV>, atom: &'static str) -> Option<Tag> {
    match state.scope.get(atom) {
        Some(SV::Tag(t)) => Some(t),
        _ => None,
    }
}
fn get_list(state: &State<SV>, atom: &'static str) -> Vec<SV> {
    match state.scope.get(atom) {
        Some(SV::List(l)) => l,
        _ => Vec::new(),
    }
}

fn hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// SnbtGrammar.createParser over NBT.
pub fn grammar() -> Grammar<SV> {
    let mut g: Grammar<SV> = Grammar::new();
    g.put_term(
        "sign",
        alt(vec![
            seq(vec![ch('+'), Term::Marker("sign", SV::Sign(false))]),
            seq(vec![ch('-'), Term::Marker("sign", SV::Sign(true))]),
        ]),
        |s| s.scope.get("sign"),
    );
    let suffix = |signed: Option<bool>, ty: TypeSuffix| {
        Term::Marker("integer_suffix", SV::Suffix(signed, Some(ty)))
    };
    let types = |signed: Option<bool>| {
        alt(vec![
            seq(vec![chs('b', 'B'), suffix(signed, TypeSuffix::Byte)]),
            seq(vec![chs('s', 'S'), suffix(signed, TypeSuffix::Short)]),
            seq(vec![chs('i', 'I'), suffix(signed, TypeSuffix::Int)]),
            seq(vec![chs('l', 'L'), suffix(signed, TypeSuffix::Long)]),
        ])
    };
    g.put_term(
        "integer_suffix",
        alt(vec![
            seq(vec![chs('u', 'U'), types(Some(false))]),
            seq(vec![chs('s', 'S'), types(Some(true))]),
            seq(vec![chs('b', 'B'), suffix(None, TypeSuffix::Byte)]),
            seq(vec![chs('s', 'S'), suffix(None, TypeSuffix::Short)]),
            seq(vec![chs('i', 'I'), suffix(None, TypeSuffix::Int)]),
            seq(vec![chs('l', 'L'), suffix(None, TypeSuffix::Long)]),
        ]),
        |s| s.scope.get("integer_suffix"),
    );
    g.put_custom("binary_numeral", |s| {
        number_run(
            s,
            |c| matches!(c, '0' | '1' | '_'),
            "snbt.parser.expected_binary_numeral",
        )
    });
    g.put_custom("decimal_numeral", |s| {
        number_run(
            s,
            |c| c.is_ascii_digit() || c == '_',
            "snbt.parser.expected_decimal_numeral",
        )
    });
    g.put_custom("hex_numeral", |s| {
        number_run(
            s,
            |c| c.is_ascii_hexdigit() || c == '_',
            "snbt.parser.expected_hex_numeral",
        )
    });
    let sign = g.named("sign");
    let hex_numeral = g.named("hex_numeral");
    let binary_numeral = g.named("binary_numeral");
    let decimal = g.named("decimal_numeral");
    let decimal2 = g.named("decimal_numeral");
    let isuffix = g.named("integer_suffix");
    g.put_term(
        "integer_literal",
        seq(vec![
            opt(sign),
            alt(vec![
                seq(vec![
                    ch('0'),
                    Term::Cut,
                    alt(vec![
                        seq(vec![chs('x', 'X'), Term::Cut, hex_numeral]),
                        seq(vec![chs('b', 'B'), binary_numeral]),
                        seq(vec![
                            decimal,
                            Term::Cut,
                            Term::Fail(delayed("snbt.parser.leading_zero_not_allowed")),
                        ]),
                        Term::Marker("decimal_numeral", SV::Str("0".into())),
                    ]),
                ]),
                decimal2,
            ]),
            opt(isuffix),
        ]),
        |s| {
            let (signed, ty) = match s.scope.get("integer_suffix") {
                Some(SV::Suffix(a, b)) => (a, b),
                _ => (None, None),
            };
            let minus = matches!(s.scope.get("sign"), Some(SV::Sign(true)));
            let (base, digits) = if let Some(d) = get_str(s, "decimal_numeral") {
                (Base::Decimal, d)
            } else if let Some(h) = get_str(s, "hex_numeral") {
                (Base::Hex, h)
            } else {
                (
                    Base::Binary,
                    get_str(s, "binary_numeral").unwrap_or_default(),
                )
            };
            Some(SV::Int(IntLit {
                minus,
                base,
                digits,
                signed,
                ty,
            }))
        },
    );
    g.put_term(
        "float_type_suffix",
        alt(vec![
            seq(vec![
                chs('f', 'F'),
                Term::Marker("float_type_suffix", SV::Type(TypeSuffix::Float)),
            ]),
            seq(vec![
                chs('d', 'D'),
                Term::Marker("float_type_suffix", SV::Type(TypeSuffix::Double)),
            ]),
        ]),
        |s| s.scope.get("float_type_suffix"),
    );
    let sign = g.named("sign");
    let decimal = g.named("decimal_numeral");
    g.put_term(
        "float_exponent_part",
        seq(vec![chs('e', 'E'), opt(sign), decimal]),
        |s| {
            let minus = matches!(s.scope.get("sign"), Some(SV::Sign(true)));
            Some(SV::Exp(
                minus,
                get_str(s, "decimal_numeral").unwrap_or_default(),
            ))
        },
    );
    let sign = g.named("sign");
    let mut whole = || g.named_as("decimal_numeral", "float_whole_part");
    let (w1, w2, w3) = (whole(), whole(), whole());
    let f1 = g.named_as("decimal_numeral", "float_fraction_part");
    let f2 = g.named_as("decimal_numeral", "float_fraction_part");
    let mut exp = || g.named("float_exponent_part");
    let (e1, e2, e3, e4) = (exp(), exp(), exp(), exp());
    let mut fs = || g.named("float_type_suffix");
    let (s1, s2, s3, s4) = (fs(), fs(), fs(), fs());
    g.put_term(
        "float_literal",
        seq(vec![
            opt(sign),
            alt(vec![
                seq(vec![w1, ch('.'), Term::Cut, opt(f1), opt(e1), opt(s1)]),
                seq(vec![ch('.'), Term::Cut, f2, opt(e2), opt(s2)]),
                seq(vec![w2, e3, Term::Cut, opt(s3)]),
                seq(vec![w3, opt(e4), s4]),
            ]),
        ]),
        |s| {
            let minus = matches!(s.scope.get("sign"), Some(SV::Sign(true)));
            let mut text = String::new();
            if minus {
                text.push('-');
            }
            if let Some(w) = get_str(s, "float_whole_part") {
                text.extend(w.chars().filter(|c| *c != '_'));
            }
            if let Some(f) = get_str(s, "float_fraction_part") {
                text.push('.');
                text.extend(f.chars().filter(|c| *c != '_'));
            }
            if let Some(SV::Exp(m, d)) = s.scope.get("float_exponent_part") {
                text.push('e');
                if m {
                    text.push('-');
                }
                text.extend(d.chars().filter(|c| *c != '_'));
            }
            let ty = match s.scope.get("float_type_suffix") {
                Some(SV::Type(t)) => Some(t),
                _ => None,
            };
            float_tag(&text, ty, s).map(SV::Tag)
        },
    );
    for (name, n) in [
        ("string_hex_2", 2),
        ("string_hex_4", 4),
        ("string_hex_8", 8),
    ] {
        g.put_custom(name, move |s| {
            greedy(
                s,
                hex,
                n,
                n,
                Reason::Delayed("snbt.parser.expected_hex_escape", vec![n.to_string()]),
            )
        });
    }
    g.put_custom("string_unicode_name", |s| {
        greedy(
            s,
            |c| c.is_ascii_alphanumeric() || c == '-' || c == ' ',
            1,
            usize::MAX,
            delayed("snbt.parser.invalid_character_name"),
        )
    });
    let esc = |c: char, v: &str| {
        seq(vec![
            ch(c),
            Term::Marker("string_escape_sequence", SV::Str(v.into())),
        ])
    };
    let h2 = g.named("string_hex_2");
    let h4 = g.named("string_hex_4");
    let h8 = g.named("string_hex_8");
    let un = g.named("string_unicode_name");
    g.put_term(
        "string_escape_sequence",
        alt(vec![
            esc('b', "\u{8}"),
            esc('s', " "),
            esc('t', "\t"),
            esc('n', "\n"),
            esc('f', "\u{c}"),
            esc('r', "\r"),
            esc('\\', "\\"),
            esc('\'', "'"),
            esc('"', "\""),
            seq(vec![ch('x'), h2]),
            seq(vec![ch('u'), h4]),
            seq(vec![ch('U'), h8]),
            seq(vec![ch('N'), ch('{'), un, ch('}')]),
        ]),
        |s| {
            if let Some(SV::Str(plain)) = s.scope.get_any(&["string_escape_sequence"]) {
                return Some(SV::Str(plain));
            }
            if let Some(SV::Str(h)) =
                s.scope
                    .get_any(&["string_hex_2", "string_hex_4", "string_hex_8"])
            {
                let cp = u32::from_str_radix(&h, 16).unwrap_or(u32::MAX);
                if cp > 0x10ffff {
                    s.store_at_mark(Reason::Delayed(
                        "snbt.parser.invalid_codepoint",
                        vec![format!("U+{cp:08X}")],
                    ));
                    return None;
                }
                return Some(SV::Str(
                    char::from_u32(cp).unwrap_or('\u{fffd}').to_string(),
                ));
            }
            // Character.codePointOf: Unicode names are not tabulated here.
            s.store_at_mark(delayed("snbt.parser.invalid_character_name"));
            None
        },
    );
    g.put_custom("string_plain_contents", |s| {
        greedy(
            s,
            |c| !matches!(c, '"' | '\'' | '\\'),
            1,
            usize::MAX,
            delayed("snbt.parser.invalid_string_contents"),
        )
    });
    for (chunk, contents, other) in [
        (
            "single_quoted_string_chunk",
            "single_quoted_string_contents",
            '"',
        ),
        (
            "double_quoted_string_chunk",
            "double_quoted_string_contents",
            '\'',
        ),
    ] {
        let plain = g.named_as("string_plain_contents", "string_contents");
        let escape = g.named_as("string_escape_sequence", "string_contents");
        g.put_term(
            chunk,
            alt(vec![
                plain,
                seq(vec![ch('\\'), escape]),
                seq(vec![
                    ch(other),
                    Term::Marker("string_contents", SV::Str(other.to_string())),
                ]),
            ]),
            |s| s.scope.get("string_contents"),
        );
        let rule = g.id(chunk);
        g.put_term(
            contents,
            Term::Repeated {
                rule,
                list: "string_chunks",
                min: 0,
            },
            |s| {
                let mut out = String::new();
                for v in get_list(s, "string_chunks") {
                    if let SV::Str(x) = v {
                        out.push_str(&x);
                    }
                }
                Some(SV::Str(out))
            },
        );
    }
    let dq = g.named_as("double_quoted_string_contents", "string_contents");
    let sq = g.named_as("single_quoted_string_contents", "string_contents");
    g.put_term(
        "quoted_string_literal",
        alt(vec![
            seq(vec![ch('"'), Term::Cut, opt(dq), ch('"')]),
            seq(vec![ch('\''), opt(sq), ch('\'')]),
        ]),
        |s| s.scope.get("string_contents"),
    );
    g.put_custom("unquoted_string", |s| {
        let mut r = s.reader();
        r.skip_whitespace();
        s.set_reader(&r);
        let cursor = s.cursor;
        let v = r.read_unquoted_string();
        s.set_reader(&r);
        if v.is_empty() {
            s.store(
                cursor,
                Sugg::Empty,
                delayed("snbt.parser.expected_unquoted_string"),
            );
            return None;
        }
        Some(SV::Str(v.to_owned()))
    });
    let literal = g.id("literal");
    g.put_term(
        "arguments",
        Term::RepeatedSep {
            rule: literal,
            list: "arguments",
            sep: Box::new(ch(',')),
            min: 0,
            trailing: true,
        },
        |s| s.scope.get("arguments"),
    );
    let uq = g.named("unquoted_string");
    let args = g.named("arguments");
    g.put_term(
        "unquoted_string_or_builtin",
        seq(vec![uq, opt(seq(vec![ch('('), args, ch(')')]))]),
        |s| {
            let contents = get_str(s, "unquoted_string").unwrap_or_default();
            let first = contents.chars().next();
            if first.is_some_and(|c| !can_start_number(c)) {
                if let Some(SV::List(arguments)) = s.scope.get("arguments") {
                    return builtin(&contents, arguments, s);
                }
                if contents.eq_ignore_ascii_case("true") {
                    return Some(SV::Tag(Tag::Byte(1)));
                }
                if contents.eq_ignore_ascii_case("false") {
                    return Some(SV::Tag(Tag::Byte(0)));
                }
                return Some(SV::Tag(Tag::String(JavaString::from(contents.as_str()))));
            }
            let ids = ["false", "true", "bool", "uuid"].map(String::from).to_vec();
            s.store(
                s.cursor,
                Sugg::Values(Rc::new(ids)),
                delayed("snbt.parser.invalid_unquoted_start"),
            );
            None
        },
    );
    let qs = g.named("quoted_string_literal");
    let uq = g.named("unquoted_string");
    g.put_term("map_key", alt(vec![qs, uq]), |s| {
        s.scope
            .get_any(&["quoted_string_literal", "unquoted_string"])
    });
    let key = g.named("map_key");
    let lit = g.named("literal");
    g.put_term("map_entry", seq(vec![key, ch(':'), lit]), |s| {
        let key = get_str(s, "map_key").unwrap_or_default();
        if key.is_empty() {
            s.store_at_mark(delayed("snbt.parser.empty_key"));
            return None;
        }
        let value = get_tag(s, "literal")?;
        Some(SV::Entry(key, value))
    });
    let entry = g.id("map_entry");
    g.put_term(
        "map_entries",
        Term::RepeatedSep {
            rule: entry,
            list: "map_entries",
            sep: Box::new(ch(',')),
            min: 0,
            trailing: true,
        },
        |s| s.scope.get("map_entries"),
    );
    let entries = g.named("map_entries");
    g.put_term("map_literal", seq(vec![ch('{'), entries, ch('}')]), |s| {
        let mut map = Compound::new();
        for e in get_list(s, "map_entries") {
            if let SV::Entry(k, v) = e {
                map.insert(JavaString::from(k.as_str()), v);
            }
        }
        Some(SV::Tag(Tag::Compound(map)))
    });
    g.put_term(
        "list_entries",
        Term::RepeatedSep {
            rule: literal,
            list: "list_entries",
            sep: Box::new(ch(',')),
            min: 0,
            trailing: true,
        },
        |s| s.scope.get("list_entries"),
    );
    g.put_term(
        "array_prefix",
        alt(vec![
            seq(vec![
                ch('B'),
                Term::Marker("array_prefix", SV::Prefix(ArrayPrefix::Byte)),
            ]),
            seq(vec![
                ch('L'),
                Term::Marker("array_prefix", SV::Prefix(ArrayPrefix::Long)),
            ]),
            seq(vec![
                ch('I'),
                Term::Marker("array_prefix", SV::Prefix(ArrayPrefix::Int)),
            ]),
        ]),
        |s| s.scope.get("array_prefix"),
    );
    let int_lit = g.id("integer_literal");
    g.put_term(
        "int_array_entries",
        Term::RepeatedSep {
            rule: int_lit,
            list: "int_array_entries",
            sep: Box::new(ch(',')),
            min: 0,
            trailing: true,
        },
        |s| s.scope.get("int_array_entries"),
    );
    let prefix = g.named("array_prefix");
    let ia = g.named("int_array_entries");
    let le = g.named("list_entries");
    g.put_term(
        "list_literal",
        seq(vec![
            ch('['),
            alt(vec![seq(vec![prefix, ch(';'), ia]), le]),
            ch(']'),
        ]),
        |s| {
            if let Some(SV::Prefix(p)) = s.scope.get("array_prefix") {
                let entries = get_list(s, "int_array_entries");
                return array(p, entries, s).map(SV::Tag);
            }
            let items: Vec<Tag> = get_list(s, "list_entries")
                .into_iter()
                .filter_map(|v| match v {
                    SV::Tag(t) => Some(t),
                    _ => None,
                })
                .collect();
            Some(SV::Tag(Tag::List(items)))
        },
    );
    let float_lit = g.named_as("float_literal", "literal");
    let int_named = g.named("integer_literal");
    let quoted = g.named("quoted_string_literal");
    let map = g.named_as("map_literal", "literal");
    let list = g.named_as("list_literal", "literal");
    let builtin_term = g.named_as("unquoted_string_or_builtin", "literal");
    g.put_term(
        "literal",
        alt(vec![
            seq(vec![
                look(Term::Chars {
                    values: Vec::new(),
                    accept: Some(can_start_number),
                }),
                alt(vec![float_lit, int_named]),
            ]),
            seq(vec![look(chs('"', '\'')), Term::Cut, quoted]),
            seq(vec![look(ch('{')), Term::Cut, map]),
            seq(vec![look(ch('[')), Term::Cut, list]),
            builtin_term,
        ]),
        |s| {
            if let Some(q) = get_str(s, "quoted_string_literal") {
                return Some(SV::Tag(Tag::String(JavaString::from(q.as_str()))));
            }
            if let Some(SV::Int(i)) = s.scope.get("integer_literal") {
                let ty = i.ty.unwrap_or(TypeSuffix::Int);
                return i.create(ty, s).map(SV::Tag);
            }
            s.scope.get("literal")
        },
    );
    g.top = literal;
    g
}

fn array(p: ArrayPrefix, entries: Vec<SV>, s: &mut State<SV>) -> Option<Tag> {
    let (default, allowed): (TypeSuffix, &[TypeSuffix]) = match p {
        ArrayPrefix::Byte => (TypeSuffix::Byte, &[]),
        ArrayPrefix::Int => (TypeSuffix::Int, &[TypeSuffix::Byte, TypeSuffix::Short]),
        ArrayPrefix::Long => (
            TypeSuffix::Long,
            &[TypeSuffix::Byte, TypeSuffix::Short, TypeSuffix::Int],
        ),
    };
    let mut values: Vec<i64> = Vec::new();
    for e in entries {
        let SV::Int(lit) = e else { continue };
        let ty = match lit.ty {
            None => default,
            Some(t) if t == default || allowed.contains(&t) => t,
            Some(_) => {
                s.store_at_mark(delayed("snbt.parser.invalid_array_element_type"));
                return None;
            }
        };
        let v = match lit.create(ty, s)? {
            Tag::Byte(v) => i64::from(v),
            Tag::Short(v) => i64::from(v),
            Tag::Int(v) => i64::from(v),
            Tag::Long(v) => v,
            _ => 0,
        };
        values.push(v);
    }
    Some(match p {
        ArrayPrefix::Byte => Tag::ByteArray(values.iter().map(|v| *v as i8 as u8).collect()),
        ArrayPrefix::Int => Tag::IntArray(values.iter().map(|v| *v as i32).collect()),
        ArrayPrefix::Long => Tag::LongArray(values),
    })
}

fn builtin(id: &str, arguments: Vec<SV>, s: &mut State<SV>) -> Option<SV> {
    let args: Vec<Tag> = arguments
        .into_iter()
        .filter_map(|v| match v {
            SV::Tag(t) => Some(t),
            _ => None,
        })
        .collect();
    match (id, args.len()) {
        ("bool", 1) => {
            let b = match &args[0] {
                Tag::Byte(v) => Some(*v != 0),
                Tag::Short(v) => Some(*v != 0),
                Tag::Int(v) => Some(*v != 0),
                Tag::Long(v) => Some(*v != 0),
                Tag::Float(v) => Some(f32::from_bits(*v) != 0.0),
                Tag::Double(v) => Some(f64::from_bits(*v) != 0.0),
                _ => None,
            };
            match b {
                Some(b) => Some(SV::Tag(Tag::Byte(i8::from(b)))),
                None => {
                    s.store_at_mark(delayed("snbt.parser.expected_number_or_boolean"));
                    None
                }
            }
        }
        ("uuid", 1) => {
            let parsed = match &args[0] {
                Tag::String(text) => super::selector::java_uuid(&text.to_string_lossy()),
                _ => None,
            };
            match parsed {
                Some(u) => Some(SV::Tag(Tag::IntArray(vec![
                    (u >> 96) as u32 as i32,
                    (u >> 64) as u32 as i32,
                    (u >> 32) as u32 as i32,
                    u as u32 as i32,
                ]))),
                None => {
                    s.store_at_mark(delayed("snbt.parser.expected_string_uuid"));
                    None
                }
            }
        }
        _ => {
            s.store_at_mark(Reason::Delayed(
                "snbt.parser.no_such_operation",
                vec![format!("{id}/{}", args.len())],
            ));
            None
        }
    }
}

thread_local! {
    static GRAMMAR: Grammar<SV> = grammar();
}

/// TagParser.parseAsArgument: one SNBT value at the reader's cursor.
pub fn parse_tag_argument(reader: &mut Reader) -> CmdResult<Tag> {
    GRAMMAR.with(|g| match g.parse_for_commands(reader)? {
        SV::Tag(t) => Ok(t),
        _ => Err(CommandError::tr_at(
            "argument.nbt.expected.compound",
            &[],
            reader,
        )),
    })
}
/// TagParser.parseCompoundAsArgument.
pub fn parse_compound_argument(reader: &mut Reader) -> CmdResult<Compound> {
    match parse_tag_argument(reader)? {
        Tag::Compound(c) => Ok(c),
        _ => Err(CommandError::tr_at(
            "argument.nbt.expected.compound",
            &[],
            reader,
        )),
    }
}
/// TagParser.parseFully.
pub fn parse_fully(text: &str) -> CmdResult<Tag> {
    let mut reader = Reader::new(text);
    let tag = parse_tag_argument(&mut reader)?;
    reader.skip_whitespace();
    if reader.can_read() {
        return Err(CommandError::tr_at("argument.nbt.trailing", &[], &reader));
    }
    Ok(tag)
}
/// The grammar's completion suggestions (ParserBasedArgument.listSuggestions).
pub fn suggest(builder: &mut SuggestionsBuilder) {
    GRAMMAR.with(|g| g.parse_for_suggestions(builder));
}
/// Parse with an outer state (TagParseRule inside another grammar).
pub fn rule_in<V: super::packrat::Value>(
    state: &mut State<V>,
    wrap: impl Fn(Tag) -> V,
) -> Option<V> {
    let mut r = state.reader();
    r.skip_whitespace();
    state.set_reader(&r);
    let mark = state.cursor;
    let mut reader = state.reader();
    match parse_tag_argument(&mut reader) {
        Ok(t) => {
            state.set_reader(&reader);
            Some(wrap(t))
        }
        Err(e) => {
            state.store(mark, Sugg::Empty, Reason::Error(e));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_fully;

    /// A crafted over-deep value must be rejected, never overflow the stack.
    #[test]
    fn deeply_nested_compound_is_rejected_not_overflowed() {
        let mut deep = String::from("1");
        for _ in 0..400 {
            deep = format!("{{a:{deep}}}");
        }
        assert!(parse_fully(&deep).is_err());
    }

    /// The `extra` chain of a JSON text component is the same SNBT shape.
    #[test]
    fn deeply_nested_list_is_rejected_not_overflowed() {
        let mut deep = String::from("\"x\"");
        for _ in 0..400 {
            deep = format!("[{deep}]");
        }
        assert!(parse_fully(&deep).is_err());
    }

    /// Shallow, real command values must still parse (the cap is not too low).
    #[test]
    fn shallow_values_still_parse() {
        assert!(parse_fully("{a:{b:{c:1}}}").is_ok());
        assert!(parse_fully("[[1,2],[3,4]]").is_ok());
    }

    /// Deep but real values parse: 50 nested compounds, and an item whose
    /// `container` holds an item with a `container`, ten levels down.
    #[test]
    fn deep_real_values_still_parse() {
        let mut compound = String::from("1");
        for _ in 0..50 {
            compound = format!("{{a:{compound}}}");
        }
        assert!(parse_fully(&compound).is_ok());
        let mut item = String::from("{id:\"minecraft:stone\",count:1}");
        for _ in 0..10 {
            item = format!(
                "{{id:\"minecraft:shulker_box\",count:1,components:{{\"minecraft:container\":[{{slot:0,item:{item}}}]}}}}"
            );
        }
        assert!(parse_fully(&item).is_ok());
    }
}
