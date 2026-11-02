// Copyright (c) 2025 Mathieu Dutour Sikiric
// SPDX-License-Identifier: MIT OR Apache-2.0
// Horizon compatibility changes are documented in ../HORIZON.md.

//! Generator-friendly AST for Java regex patterns, used by fuzzers and property tests.
//!
//! This AST is intentionally separate from the parser's internal `Node` type:
//! generators care about *renderable* shapes, not engine state. Render with
//! [`render`] to get a pattern string suitable for [`crate::Regex::new`].
//!
//! Gated behind the `fuzz-gen` feature so production users don't pay for the
//! optional `arbitrary` dependency.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Top-level regex AST node.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum RegexNode {
    Literal(LitChar),
    Dot,
    Anchor(Anchor),
    Escape(EscClass),
    LineBreak, // \R
    Class(CharClass),
    Concat(Vec<RegexNode>),
    Alt(Vec<RegexNode>),
    Group {
        kind: GroupKind,
        body: Box<RegexNode>,
    },
    Quantified {
        body: Box<RegexNode>,
        quant: Quantifier,
    },
    Lookaround {
        ahead: bool,
        neg: bool,
        body: Box<RegexNode>,
    },
    Backref(BackrefIdx),
    Quote(Vec<LitChar>),  // \Q...\E
    InlineFlags(FlagSet), // (?i-m) etc.
    FlagGroup {
        flags: FlagSet,
        body: Box<RegexNode>,
    }, // (?i:...)
}

/// A character intended as a literal. Restricted to a tractable subset for
/// fuzzing; the renderer escapes metacharacters as needed.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum LitChar {
    Ascii(AsciiPrintable),
    Newline,
    Tab,
    Cr,
    Unicode(UnicodeChar),
}

/// Sample of common ASCII printable characters. Avoids null bytes and other
/// weirdness that complicates rendering.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum AsciiPrintable {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    Zero,
    One,
    Two,
    Three,
    Space,
    Comma,
    Colon,
    Underscore,
    Slash,
    Hyphen,
    At,
    OpenAngle,
    CloseAngle,
    Equals,
    Plus,
    Tilde,
}

/// A handful of non-ASCII code points spanning interesting Unicode behaviors
/// (Latin diacritics, Greek, CJK, an emoji that requires a surrogate pair in Java).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum UnicodeChar {
    LatinEAcute,   // é U+00E9
    LatinSsharp,   // ß U+00DF (folds to "ss")
    GreekAlpha,    // α U+03B1
    GreekCapAlpha, // Α U+0391
    CyrillicYa,    // я U+044F
    Cjk,           // 中 U+4E2D
    Snowman,       // ☃ U+2603
    EmojiGrin,     // 😀 U+1F600 (supplementary plane)
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum Anchor {
    StartLine,       // ^
    EndLine,         // $
    StartInput,      // \A
    EndInputZ,       // \z
    EndInputBigZ,    // \Z
    WordBoundary,    // \b
    NonWordBoundary, // \B
    PrevMatchEnd,    // \G
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum EscClass {
    Digit,
    NonDigit,
    Word,
    NonWord,
    Space,
    NonSpace,
    HSpace,
    NonHSpace,
    VSpace,
    NonVSpace,
    UnicodeLetter,    // \p{L}
    UnicodeNotLetter, // \P{L}
    UnicodeDigit,     // \p{Nd}
    UnicodePunct,     // \p{P}
    // Java-specific property aliases — sample a few to exercise the parser's
    // alias-resolution path. All of these should be accepted by both engines.
    JavaUpperCase,  // \p{javaUpperCase}
    JavaWhitespace, // \p{javaWhitespace}
    InGreek,        // \p{InGreek}     — block alias form
    IsLatin,        // \p{IsLatin}     — script alias form
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub struct CharClass {
    pub negated: bool,
    pub items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum ClassItem {
    Single(LitChar),
    Range(AsciiPrintable, AsciiPrintable), // ordered before render
    Esc(EscClass),
    Nested(Box<CharClass>),
    Intersect(Box<CharClass>, Box<CharClass>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum GroupKind {
    Capturing,
    NonCapturing,
    Atomic,
    Named(GroupName),
}

/// A small set of valid group names (must match Java's `[A-Za-z][A-Za-z0-9]*`).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum GroupName {
    Foo,
    Bar,
    Baz,
    X1,
    Y2,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub struct Quantifier {
    pub kind: QuantKind,
    pub mode: QuantMode,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum QuantKind {
    Star,
    Plus,
    Opt,
    Exact(SmallCount),
    AtLeast(SmallCount),
    Range(SmallCount, SmallCount),
}

/// Bounded small counts — keeps generated patterns from blowing up.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum SmallCount {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
}

impl SmallCount {
    fn val(self) -> u32 {
        match self {
            SmallCount::Zero => 0,
            SmallCount::One => 1,
            SmallCount::Two => 2,
            SmallCount::Three => 3,
            SmallCount::Four => 4,
            SmallCount::Five => 5,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum QuantMode {
    Greedy,
    Reluctant,
    Possessive,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub enum BackrefIdx {
    B1,
    B2,
    B3,
}

#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "fuzz-gen", derive(arbitrary::Arbitrary))]
pub struct FlagSet {
    pub i: bool,
    pub m: bool,
    pub s: bool,
    pub u: bool,
}

impl LitChar {
    pub fn to_char(self) -> char {
        match self {
            LitChar::Ascii(a) => a.to_char(),
            LitChar::Newline => '\n',
            LitChar::Tab => '\t',
            LitChar::Cr => '\r',
            LitChar::Unicode(u) => u.to_char(),
        }
    }
}

impl AsciiPrintable {
    pub fn to_char(self) -> char {
        match self {
            AsciiPrintable::A => 'a',
            AsciiPrintable::B => 'b',
            AsciiPrintable::C => 'c',
            AsciiPrintable::D => 'd',
            AsciiPrintable::E => 'e',
            AsciiPrintable::F => 'f',
            AsciiPrintable::G => 'g',
            AsciiPrintable::H => 'h',
            AsciiPrintable::Zero => '0',
            AsciiPrintable::One => '1',
            AsciiPrintable::Two => '2',
            AsciiPrintable::Three => '3',
            AsciiPrintable::Space => ' ',
            AsciiPrintable::Comma => ',',
            AsciiPrintable::Colon => ':',
            AsciiPrintable::Underscore => '_',
            AsciiPrintable::Slash => '/',
            AsciiPrintable::Hyphen => '-',
            AsciiPrintable::At => '@',
            AsciiPrintable::OpenAngle => '<',
            AsciiPrintable::CloseAngle => '>',
            AsciiPrintable::Equals => '=',
            AsciiPrintable::Plus => '+',
            AsciiPrintable::Tilde => '~',
        }
    }
}

impl UnicodeChar {
    pub fn to_char(self) -> char {
        match self {
            UnicodeChar::LatinEAcute => 'é',
            UnicodeChar::LatinSsharp => 'ß',
            UnicodeChar::GreekAlpha => 'α',
            UnicodeChar::GreekCapAlpha => 'Α',
            UnicodeChar::CyrillicYa => 'я',
            UnicodeChar::Cjk => '中',
            UnicodeChar::Snowman => '☃',
            UnicodeChar::EmojiGrin => '\u{1F600}',
        }
    }
}

impl GroupName {
    pub fn name(self) -> &'static str {
        match self {
            GroupName::Foo => "foo",
            GroupName::Bar => "bar",
            GroupName::Baz => "baz",
            GroupName::X1 => "x1",
            GroupName::Y2 => "y2",
        }
    }
}

impl FlagSet {
    /// Render as `iXX` chars for inline flag groups.
    fn to_chars(self) -> String {
        let mut s = String::new();
        if self.i {
            s.push('i');
        }
        if self.m {
            s.push('m');
        }
        if self.s {
            s.push('s');
        }
        if self.u {
            s.push('u');
        }
        s
    }
    pub fn to_flags_str(self) -> String {
        self.to_chars()
    }
}

/// Render an AST node to a regex pattern string. Always produces UTF-8 output
/// and (modulo Java compile-time validation) a string acceptable to the parser.
pub fn render(node: &RegexNode) -> String {
    let mut out = String::new();
    render_into(node, &mut out);
    out
}

fn render_into(node: &RegexNode, out: &mut String) {
    match node {
        RegexNode::Literal(c) => write_literal(c.to_char(), out),
        RegexNode::Dot => out.push('.'),
        RegexNode::Anchor(a) => out.push_str(anchor_str(*a)),
        RegexNode::Escape(e) => out.push_str(esc_str(*e)),
        RegexNode::LineBreak => out.push_str("\\R"),
        RegexNode::Class(c) => render_class(c, out),
        RegexNode::Concat(items) => {
            for n in items {
                render_atom(n, out);
            }
        }
        RegexNode::Alt(branches) => {
            if branches.is_empty() {
                // empty alt would be invalid; render as empty group
                out.push_str("(?:)");
                return;
            }
            for (i, b) in branches.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                render_branch(b, out);
            }
        }
        RegexNode::Group { kind, body } => {
            out.push('(');
            match kind {
                GroupKind::Capturing => {}
                GroupKind::NonCapturing => out.push_str("?:"),
                GroupKind::Atomic => out.push_str("?>"),
                GroupKind::Named(n) => {
                    out.push_str("?<");
                    out.push_str(n.name());
                    out.push('>');
                }
            }
            render_branch(body, out);
            out.push(')');
        }
        RegexNode::Quantified { body, quant } => {
            render_atom(body, out);
            quant_str(quant, out);
        }
        RegexNode::Lookaround { ahead, neg, body } => {
            out.push('(');
            match (ahead, neg) {
                (true, false) => out.push_str("?="),
                (true, true) => out.push_str("?!"),
                (false, false) => out.push_str("?<="),
                (false, true) => out.push_str("?<!"),
            }
            render_branch(body, out);
            out.push(')');
        }
        RegexNode::Backref(b) => {
            let n = match b {
                BackrefIdx::B1 => 1,
                BackrefIdx::B2 => 2,
                BackrefIdx::B3 => 3,
            };
            write!(out, "\\{}", n).unwrap();
        }
        RegexNode::Quote(chars) => {
            out.push_str("\\Q");
            for c in chars {
                // \Q..\E quotes everything verbatim except the literal sequence \E itself.
                let ch = c.to_char();
                if ch == '\\' {
                    out.push('\\');
                } else {
                    out.push(ch);
                }
            }
            out.push_str("\\E");
        }
        RegexNode::InlineFlags(f) => {
            let s = f.to_chars();
            if !s.is_empty() {
                out.push_str("(?");
                out.push_str(&s);
                out.push(')');
            }
        }
        RegexNode::FlagGroup { flags, body } => {
            let s = flags.to_chars();
            if s.is_empty() {
                out.push_str("(?:");
            } else {
                out.push_str("(?");
                out.push_str(&s);
                out.push(':');
            }
            render_branch(body, out);
            out.push(')');
        }
    }
}

/// Render a node that's being used as the body of a group or branch — these
/// allow top-level alternation, so we just delegate to `render_into`.
fn render_branch(node: &RegexNode, out: &mut String) {
    render_into(node, out);
}

/// Render a node that *must* be a single atom (e.g. left of a quantifier).
/// Wraps alternations and concatenations in a non-capturing group.
fn render_atom(node: &RegexNode, out: &mut String) {
    match node {
        RegexNode::Alt(_) | RegexNode::Concat(_) => {
            out.push_str("(?:");
            render_into(node, out);
            out.push(')');
        }
        // Quantifying a quantified node would produce e.g. `a**` which Java rejects.
        // Wrap defensively.
        RegexNode::Quantified { .. } => {
            out.push_str("(?:");
            render_into(node, out);
            out.push(')');
        }
        _ => render_into(node, out),
    }
}

fn anchor_str(a: Anchor) -> &'static str {
    match a {
        Anchor::StartLine => "^",
        Anchor::EndLine => "$",
        Anchor::StartInput => "\\A",
        Anchor::EndInputZ => "\\z",
        Anchor::EndInputBigZ => "\\Z",
        Anchor::WordBoundary => "\\b",
        Anchor::NonWordBoundary => "\\B",
        Anchor::PrevMatchEnd => "\\G",
    }
}

fn esc_str(e: EscClass) -> &'static str {
    match e {
        EscClass::Digit => "\\d",
        EscClass::NonDigit => "\\D",
        EscClass::Word => "\\w",
        EscClass::NonWord => "\\W",
        EscClass::Space => "\\s",
        EscClass::NonSpace => "\\S",
        EscClass::HSpace => "\\h",
        EscClass::NonHSpace => "\\H",
        EscClass::VSpace => "\\v",
        EscClass::NonVSpace => "\\V",
        EscClass::UnicodeLetter => "\\p{L}",
        EscClass::UnicodeNotLetter => "\\P{L}",
        EscClass::UnicodeDigit => "\\p{Nd}",
        EscClass::UnicodePunct => "\\p{P}",
        EscClass::JavaUpperCase => "\\p{javaUpperCase}",
        EscClass::JavaWhitespace => "\\p{javaWhitespace}",
        EscClass::InGreek => "\\p{InGreek}",
        EscClass::IsLatin => "\\p{IsLatin}",
    }
}

fn quant_str(q: &Quantifier, out: &mut String) {
    match q.kind {
        QuantKind::Star => out.push('*'),
        QuantKind::Plus => out.push('+'),
        QuantKind::Opt => out.push('?'),
        QuantKind::Exact(n) => write!(out, "{{{}}}", n.val()).unwrap(),
        QuantKind::AtLeast(n) => write!(out, "{{{},}}", n.val()).unwrap(),
        QuantKind::Range(lo, hi) => {
            let (a, b) = if lo.val() <= hi.val() {
                (lo.val(), hi.val())
            } else {
                (hi.val(), lo.val())
            };
            write!(out, "{{{},{}}}", a, b).unwrap();
        }
    }
    match q.mode {
        QuantMode::Greedy => {}
        QuantMode::Reluctant => out.push('?'),
        QuantMode::Possessive => out.push('+'),
    }
}

/// Escape a literal character for use *outside* a character class.
fn write_literal(c: char, out: &mut String) {
    match c {
        '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '|' | '^' | '$' => {
            out.push('\\');
            out.push(c);
        }
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        _ => out.push(c),
    }
}

/// Escape a character for use inside `[...]`. Different rules than outside:
/// only `\`, `]`, `^` (at start), `-` (between chars) need care.
fn write_class_char(c: char, out: &mut String) {
    match c {
        '\\' | ']' | '[' | '^' | '-' | '&' => {
            out.push('\\');
            out.push(c);
        }
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        _ => out.push(c),
    }
}

fn render_class(c: &CharClass, out: &mut String) {
    out.push('[');
    if c.negated {
        out.push('^');
    }
    if c.items.is_empty() {
        // empty class is invalid; emit a harmless placeholder
        out.push_str("\\w");
    } else {
        for it in &c.items {
            render_class_item(it, out);
        }
    }
    out.push(']');
}

fn render_class_item(it: &ClassItem, out: &mut String) {
    match it {
        ClassItem::Single(c) => write_class_char(c.to_char(), out),
        ClassItem::Range(lo, hi) => {
            let (a, b) = (lo.to_char(), hi.to_char());
            let (lo_c, hi_c) = if (a as u32) <= (b as u32) {
                (a, b)
            } else {
                (b, a)
            };
            write_class_char(lo_c, out);
            out.push('-');
            write_class_char(hi_c, out);
        }
        ClassItem::Esc(e) => out.push_str(esc_str(*e)),
        ClassItem::Nested(c) => render_class(c, out),
        ClassItem::Intersect(a, b) => {
            // Renders as `[A]&&[B]` inside the surrounding class, yielding
            // e.g. `[[a-z]&&[^aeiou]]` overall. Valid Java intersection syntax.
            render_class(a, out);
            out.push_str("&&");
            render_class(b, out);
        }
    }
}

#[cfg(all(test, feature = "fuzz-gen"))]
mod tests {
    use super::*;
    use alloc::vec;
    use arbitrary::{Arbitrary, Unstructured};

    /// Deterministic SplitMix64 — same PRNG used by diff_fuzz.
    fn fill(state: &mut u64, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            *state = state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = *state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            let bytes = (z ^ (z >> 31)).to_le_bytes();
            for (i, b) in chunk.iter_mut().enumerate() {
                *b = bytes[i];
            }
        }
    }

    #[test]
    fn fuzz_render_covers_all_variants() {
        // 50k Arbitrary-driven render() calls. The PRNG seed is fixed so
        // the coverage signal is deterministic across runs.
        let mut state: u64 = 0xDEADBEEFCAFEBABE;
        let mut buf = vec![0u8; 1024];
        let mut produced = 0usize;
        for _ in 0..50_000 {
            fill(&mut state, &mut buf);
            let mut u = Unstructured::new(&buf);
            if let Ok(node) = RegexNode::arbitrary(&mut u) {
                let s = render(&node);
                // Output must be valid UTF-8 (always true for String) and
                // non-empty for non-degenerate trees. We don't assert
                // anything about regex validity — the goal is coverage.
                let _ = s;
                produced += 1;
            }
        }
        assert!(
            produced > 1000,
            "expected >1k successful renders, got {produced}"
        );
    }

    #[test]
    fn flag_set_to_flags_str_alias() {
        // `to_flags_str` is a public alias of the private `to_chars`. Direct
        // call covers the alias method.
        let f = FlagSet {
            i: true,
            m: false,
            s: true,
            u: false,
        };
        assert_eq!(f.to_flags_str(), "is");
    }

    #[test]
    fn render_each_top_level_variant() {
        // Direct hand-rolled coverage of each top-level RegexNode variant
        // and a few representative inner shapes. Acts as a checklist that
        // every render path runs at least once.
        let cases: alloc::vec::Vec<RegexNode> = vec![
            RegexNode::Literal(LitChar::Cr),
            RegexNode::Dot,
            RegexNode::Anchor(Anchor::WordBoundary),
            RegexNode::Escape(EscClass::Digit),
            RegexNode::LineBreak,
            RegexNode::Concat(vec![RegexNode::Dot, RegexNode::Dot]),
            RegexNode::Alt(vec![RegexNode::Dot, RegexNode::Literal(LitChar::Cr)]),
            RegexNode::Alt(vec![]),
            RegexNode::Group {
                kind: GroupKind::Capturing,
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Group {
                kind: GroupKind::NonCapturing,
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Group {
                kind: GroupKind::Atomic,
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Group {
                kind: GroupKind::Named(GroupName::Foo),
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Quantified {
                body: Box::new(RegexNode::Dot),
                quant: Quantifier {
                    kind: QuantKind::Star,
                    mode: QuantMode::Greedy,
                },
            },
            RegexNode::Lookaround {
                ahead: true,
                neg: false,
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Lookaround {
                ahead: false,
                neg: true,
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::Backref(BackrefIdx::B1),
            RegexNode::Quote(vec![LitChar::Cr]),
            RegexNode::InlineFlags(FlagSet {
                i: true,
                m: false,
                s: true,
                u: false,
            }),
            RegexNode::InlineFlags(FlagSet {
                i: false,
                m: false,
                s: false,
                u: false,
            }),
            RegexNode::FlagGroup {
                flags: FlagSet {
                    i: true,
                    m: false,
                    s: false,
                    u: false,
                },
                body: Box::new(RegexNode::Dot),
            },
            RegexNode::FlagGroup {
                flags: FlagSet {
                    i: false,
                    m: false,
                    s: false,
                    u: false,
                },
                body: Box::new(RegexNode::Dot),
            },
        ];
        for n in &cases {
            let _ = render(n);
        }
    }
}
