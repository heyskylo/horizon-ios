// Copyright (c) 2025 Mathieu Dutour Sikiric
// SPDX-License-Identifier: MIT OR Apache-2.0
// Horizon compatibility changes are documented in ../HORIZON.md.

use unicode_general_category::GeneralCategory as UGC;
use unicode_script::{Script, UnicodeScript};

use crate::java_properties as jp;
use crate::types::PredefinedClass;

pub fn get_ugc(ch: char) -> UGC {
    unicode_general_category::get_general_category(ch)
}

// Back references compare both captured/input characters symmetrically.
pub fn chars_eq_ci(a: char, b: char, unicode_case: bool) -> bool {
    if a == b {
        return true;
    }
    if unicode_case {
        let a = crate::java_case::upper(a);
        let b = crate::java_case::upper(b);
        a == b || crate::java_case::lower(a) == crate::java_case::lower(b)
    } else {
        a.eq_ignore_ascii_case(&b)
    }
}

// Pattern.single: a literal with no distinct simple uppercase/lowercase
// representation remains exact (e.g. ß), even if another scalar folds to it.
pub fn literal_eq_ci(input: char, pattern: char, unicode_case: bool) -> bool {
    if !unicode_case {
        return input.eq_ignore_ascii_case(&pattern);
    }
    let upper = crate::java_case::upper(pattern);
    let lower = crate::java_case::lower(upper);
    if upper == lower {
        input == pattern
    } else {
        input == lower || crate::java_case::lower(crate::java_case::upper(input)) == lower
    }
}

pub fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

pub fn is_word_char(c: char, unicode: bool) -> bool {
    if unicode {
        // Java's Unicode \w: [\p{Alpha}\p{gc=Mn}\p{gc=Me}\p{gc=Mc}\p{Digit}\p{gc=Pc}]
        jp::has(c, jp::ALPHABETIC)
            || matches!(c, '\u{200c}' | '\u{200d}')
            || matches!(
                get_ugc(c),
                UGC::DecimalNumber
                    | UGC::NonspacingMark
                    | UGC::SpacingMark
                    | UGC::EnclosingMark
                    | UGC::ConnectorPunctuation
            )
    } else {
        c.is_ascii_alphanumeric() || c == '_'
    }
}

pub fn is_linebreak(c: char) -> bool {
    matches!(
        c,
        '\n' | '\x0B' | '\x0C' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

fn is_unicode_punct(ch: char) -> bool {
    matches!(
        get_ugc(ch),
        UGC::ConnectorPunctuation
            | UGC::DashPunctuation
            | UGC::OpenPunctuation
            | UGC::ClosePunctuation
            | UGC::InitialPunctuation
            | UGC::FinalPunctuation
            | UGC::OtherPunctuation
    )
}

fn is_unicode_graph(ch: char) -> bool {
    let cat = get_ugc(ch);
    !matches!(
        cat,
        UGC::SpaceSeparator
            | UGC::LineSeparator
            | UGC::ParagraphSeparator
            | UGC::Control
            | UGC::Surrogate
            | UGC::Unassigned
    ) && !unicode_whitespace(ch)
}

fn is_unicode_print(ch: char) -> bool {
    is_unicode_graph(ch) || matches!(get_ugc(ch), UGC::SpaceSeparator)
}

fn unicode_whitespace(ch: char) -> bool {
    matches!(ch, '\u{9}'..='\u{d}' | ' ' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn unicode_hex_digit(ch: char) -> bool {
    jp::has(ch, jp::DIGIT)
        || matches!(ch, 'A'..='F' | 'a'..='f' | '\u{ff21}'..='\u{ff26}' | '\u{ff41}'..='\u{ff46}')
}

fn binary_property(name: &str, ch: char) -> Option<bool> {
    let name = name.to_ascii_lowercase();
    let flag = match name.as_str() {
        "alphabetic" => jp::ALPHABETIC,
        "lowercase" => jp::LOWERCASE,
        "uppercase" => jp::UPPERCASE,
        "letter" => jp::LETTER,
        "ideographic" => jp::IDEOGRAPHIC,
        "assigned" => jp::DEFINED,
        "emoji" => jp::EMOJI,
        "emoji_presentation" => jp::EMOJI_PRESENTATION,
        "emoji_modifier" => jp::EMOJI_MODIFIER,
        "emoji_modifier_base" => jp::EMOJI_MODIFIER_BASE,
        "emoji_component" => jp::EMOJI_COMPONENT,
        "extended_pictographic" => jp::EXTENDED_PICTOGRAPHIC,
        "whitespace" | "white_space" => return Some(unicode_whitespace(ch)),
        "joincontrol" | "join_control" => return Some(matches!(ch, '\u{200c}' | '\u{200d}')),
        "noncharactercodepoint" | "noncharacter_code_point" => {
            return Some(matches!(ch as u32, 0xfdd0..=0xfdef) || ch as u32 & 0xffff >= 0xfffe)
        }
        "hexdigit" | "hex_digit" => return Some(unicode_hex_digit(ch)),
        "titlecase" => return Some(matches!(get_ugc(ch), UGC::TitlecaseLetter)),
        "control" => return Some(matches!(get_ugc(ch), UGC::Control)),
        "punctuation" => return Some(is_unicode_punct(ch)),
        "word" => return Some(is_word_char(ch, true)),
        // Unicode POSIX aliases following Is always select Unicode semantics.
        "alpha" | "alnum" | "blank" | "cntrl" | "digit" | "graph" | "lower" | "print" | "punct"
        | "space" | "upper" | "xdigit" => return Some(match_unicode_property_ext(&name, ch, true)),
        _ => return None,
    };
    Some(jp::has(ch, flag))
}

pub fn is_posix_class(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "alpha"
            | "alnum"
            | "ascii"
            | "blank"
            | "cntrl"
            | "digit"
            | "graph"
            | "lower"
            | "print"
            | "punct"
            | "space"
            | "upper"
            | "xdigit"
            | "white_space"
    )
}

pub fn is_combining_mark(c: char) -> bool {
    let cat = get_ugc(c);
    matches!(
        cat,
        UGC::NonspacingMark | UGC::SpacingMark | UGC::EnclosingMark
    )
}

pub fn match_predefined_class(pc: PredefinedClass, ch: char, unicode: bool) -> bool {
    match pc {
        PredefinedClass::Digit => {
            if unicode {
                matches!(get_ugc(ch), UGC::DecimalNumber)
            } else {
                ch.is_ascii_digit()
            }
        }
        PredefinedClass::NonDigit => {
            if unicode {
                !matches!(get_ugc(ch), UGC::DecimalNumber)
            } else {
                !ch.is_ascii_digit()
            }
        }
        PredefinedClass::Word => is_word_char(ch, unicode),
        PredefinedClass::NonWord => !is_word_char(ch, unicode),
        PredefinedClass::Whitespace => {
            if unicode {
                unicode_whitespace(ch)
            } else {
                matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0C' | '\x0B')
            }
        }
        PredefinedClass::NonWhitespace => {
            if unicode {
                !unicode_whitespace(ch)
            } else {
                !matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0C' | '\x0B')
            }
        }
        PredefinedClass::HorizWhitespace => {
            matches!(
                ch,
                '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{180E}' | '\u{2000}'
                    ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
            )
        }
        PredefinedClass::NonHorizWhitespace => !matches!(
            ch,
            '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{180E}' | '\u{2000}'
                ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        ),
        PredefinedClass::VertWhitespace => {
            matches!(
                ch,
                '\n' | '\x0B' | '\x0C' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
            )
        }
        PredefinedClass::NonVertWhitespace => !matches!(
            ch,
            '\n' | '\x0B' | '\x0C' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
        ),
    }
}

pub fn is_valid_unicode_property(name: &str) -> bool {
    if name.starts_with("java") {
        return matches!(
            name,
            "javaLowerCase"
                | "javaUpperCase"
                | "javaTitleCase"
                | "javaDigit"
                | "javaLetter"
                | "javaLetterOrDigit"
                | "javaAlphabetic"
                | "javaWhitespace"
                | "javaSpaceChar"
                | "javaMirrored"
                | "javaDefined"
                | "javaIdentifierIgnorable"
                | "javaISOControl"
                | "javaIdeographic"
                | "javaJavaIdentifierStart"
                | "javaJavaIdentifierPart"
                | "javaUnicodeIdentifierStart"
                | "javaUnicodeIdentifierPart"
        );
    }
    if let Some(name) = name.strip_prefix("Is") {
        if binary_property(name, '\0').is_some() {
            return true;
        }
    }
    // Script names require "Is" prefix in Java (e.g. \p{IsLatin})
    if let Some(script) = name.strip_prefix("Is").or_else(|| name.strip_prefix("is")) {
        if resolve_script_name(script).is_some() {
            return true;
        }
    }
    // Block names use "In" prefix (e.g. \p{InBasicLatin})
    if let Some(block) = name.strip_prefix("In") {
        if resolve_block_name(block).is_some() {
            return true;
        }
    }
    let name = name.strip_prefix("Is").unwrap_or(name);
    let name_lower = name.to_lowercase();
    matches!(
        name_lower.as_str(),
        "l" | "letter"
            | "lu"
            | "uppercase_letter"
            | "upper"
            | "ll"
            | "lowercase_letter"
            | "lower"
            | "lc"
            | "cased_letter"
            | "lt"
            | "titlecase_letter"
            | "lm"
            | "modifier_letter"
            | "lo"
            | "other_letter"
            | "m"
            | "mark"
            | "mn"
            | "nonspacing_mark"
            | "mc"
            | "spacing_mark"
            | "me"
            | "enclosing_mark"
            | "n"
            | "number"
            | "nd"
            | "decimal_digit_number"
            | "digit"
            | "nl"
            | "letter_number"
            | "no"
            | "other_number"
            | "p"
            | "punctuation"
            | "punct"
            | "pc"
            | "connector_punctuation"
            | "pd"
            | "dash_punctuation"
            | "ps"
            | "open_punctuation"
            | "pe"
            | "close_punctuation"
            | "pi"
            | "initial_punctuation"
            | "pf"
            | "final_punctuation"
            | "po"
            | "other_punctuation"
            | "s"
            | "symbol"
            | "sm"
            | "math_symbol"
            | "sc"
            | "currency_symbol"
            | "sk"
            | "modifier_symbol"
            | "so"
            | "other_symbol"
            | "z"
            | "separator"
            | "zs"
            | "space_separator"
            | "zl"
            | "line_separator"
            | "zp"
            | "paragraph_separator"
            | "c"
            | "control"
            | "other"
            | "cc"
            | "cntrl"
            | "cf"
            | "format"
            | "co"
            | "private_use"
            | "cn"
            | "unassigned"
            | "alpha"
            | "alnum"
            | "ascii"
            | "blank"
            | "graph"
            | "print"
            | "space"
            | "white_space"
            | "xdigit"
            | "l1"
            | "latin1"
    )
}

/// Convenience wrapper: matches without UNICODE_CHARACTER_CLASS flag.
#[allow(dead_code)]
pub fn match_unicode_property(name: &str, ch: char) -> bool {
    match_unicode_property_ext(name, ch, false)
}

/// Match a Unicode property, with `unicode_class` controlling whether
/// POSIX classes use ASCII (false) or Unicode (true) definitions.
pub fn match_unicode_property_ext(name: &str, ch: char, unicode_class: bool) -> bool {
    // Character methods are pinned Java 25 observations, not the Rust compiler's Unicode version.
    let java_flag = match name {
        "javaLowerCase" => Some(jp::LOWERCASE),
        "javaUpperCase" => Some(jp::UPPERCASE),
        "javaDigit" => Some(jp::DIGIT),
        "javaLetter" => Some(jp::LETTER),
        "javaAlphabetic" => Some(jp::ALPHABETIC),
        "javaIdeographic" => Some(jp::IDEOGRAPHIC),
        "javaWhitespace" => Some(jp::WHITESPACE),
        "javaSpaceChar" => Some(jp::SPACECHAR),
        "javaDefined" => Some(jp::DEFINED),
        "javaMirrored" => Some(jp::MIRRORED),
        "javaJavaIdentifierStart" => Some(jp::JAVA_ID_START),
        "javaJavaIdentifierPart" => Some(jp::JAVA_ID_PART),
        "javaUnicodeIdentifierStart" => Some(jp::UNICODE_ID_START),
        "javaUnicodeIdentifierPart" => Some(jp::UNICODE_ID_PART),
        "javaIdentifierIgnorable" => Some(jp::IGNORABLE),
        _ => None,
    };
    if let Some(flag) = java_flag {
        return jp::has(ch, flag);
    }
    if name == "javaLetterOrDigit" {
        return jp::has(ch, jp::LETTER | jp::DIGIT);
    }
    if name == "javaTitleCase" {
        return matches!(get_ugc(ch), UGC::TitlecaseLetter);
    }
    if name == "javaISOControl" {
        return matches!(ch, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}');
    }
    if let Some(name) = name.strip_prefix("Is") {
        if let Some(result) = binary_property(name, ch) {
            return result;
        }
    }

    // Script names require "Is" prefix in Java (e.g. \p{IsLatin})
    if let Some(script) = name.strip_prefix("Is").or_else(|| name.strip_prefix("is")) {
        if let Some(expected) = resolve_script_name(script) {
            return ch.script() == expected;
        }
    }

    // Block names use "In" prefix (e.g. \p{InBasicLatin})
    if let Some(block) = name.strip_prefix("In") {
        if let Some(expected_block) = resolve_block_name(block) {
            return crate::java_blocks::contains(expected_block, ch);
        }
    }

    let name = name.strip_prefix("Is").unwrap_or(name);
    let name_lower = name.to_lowercase();

    // POSIX classes — ASCII-only by default, Unicode when UNICODE_CHARACTER_CLASS flag is set.
    // The `u` (bool) parameter below is `unicode_class`.
    let u = unicode_class;
    match name_lower.as_str() {
        "upper" => {
            return if u {
                jp::has(ch, jp::UPPERCASE)
            } else {
                ch.is_ascii_uppercase()
            }
        }
        "lower" => {
            return if u {
                jp::has(ch, jp::LOWERCASE)
            } else {
                ch.is_ascii_lowercase()
            }
        }
        "alpha" => {
            return if u {
                jp::has(ch, jp::ALPHABETIC)
            } else {
                ch.is_ascii_alphabetic()
            }
        }
        "digit" => {
            return if u {
                matches!(get_ugc(ch), UGC::DecimalNumber)
            } else {
                ch.is_ascii_digit()
            }
        }
        "alnum" => {
            return if u {
                jp::has(ch, jp::ALPHABETIC) || matches!(get_ugc(ch), UGC::DecimalNumber)
            } else {
                ch.is_ascii_alphanumeric()
            }
        }
        "ascii" => return ch.is_ascii(),
        "blank" => {
            return if u {
                ch == '\t' || matches!(get_ugc(ch), UGC::SpaceSeparator)
            } else {
                ch == ' ' || ch == '\t'
            }
        }
        "punct" => {
            return if u {
                is_unicode_punct(ch)
            } else {
                matches!(ch, '!'..='/' | ':'..='@' | '['..='`' | '{'..='~')
            }
        }
        "graph" => {
            return if u {
                is_unicode_graph(ch)
            } else {
                ch.is_ascii_graphic()
            }
        }
        "print" => {
            return if u {
                is_unicode_print(ch)
            } else {
                ch.is_ascii_graphic() || ch == ' '
            }
        }
        "cntrl" => {
            return if u {
                matches!(get_ugc(ch), UGC::Control)
            } else {
                ch.is_ascii_control()
            }
        }
        "space" | "white_space" => {
            return if u {
                unicode_whitespace(ch)
            } else {
                matches!(ch, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ')
            }
        }
        "xdigit" => {
            return if u {
                unicode_hex_digit(ch)
            } else {
                ch.is_ascii_hexdigit()
            }
        }
        "l1" | "latin1" => return (ch as u32) <= 0xFF,
        _ => {}
    }

    // Unicode General Categories — compute category once
    let cat = get_ugc(ch);
    match_ugc_category(&name_lower, cat)
}

fn match_ugc_category(name: &str, cat: UGC) -> bool {
    match name {
        "l" | "letter" => matches!(
            cat,
            UGC::UppercaseLetter
                | UGC::LowercaseLetter
                | UGC::TitlecaseLetter
                | UGC::ModifierLetter
                | UGC::OtherLetter
        ),
        "lu" | "uppercase_letter" => matches!(cat, UGC::UppercaseLetter),
        "ll" | "lowercase_letter" => matches!(cat, UGC::LowercaseLetter),
        "lc" | "cased_letter" => matches!(
            cat,
            UGC::UppercaseLetter | UGC::LowercaseLetter | UGC::TitlecaseLetter
        ),
        "lt" | "titlecase_letter" => matches!(cat, UGC::TitlecaseLetter),
        "lm" | "modifier_letter" => matches!(cat, UGC::ModifierLetter),
        "lo" | "other_letter" => matches!(cat, UGC::OtherLetter),
        "m" | "mark" => matches!(
            cat,
            UGC::NonspacingMark | UGC::SpacingMark | UGC::EnclosingMark
        ),
        "mn" | "nonspacing_mark" => matches!(cat, UGC::NonspacingMark),
        "mc" | "spacing_mark" => matches!(cat, UGC::SpacingMark),
        "me" | "enclosing_mark" => matches!(cat, UGC::EnclosingMark),
        "n" | "number" => matches!(
            cat,
            UGC::DecimalNumber | UGC::LetterNumber | UGC::OtherNumber
        ),
        "nd" | "decimal_digit_number" => matches!(cat, UGC::DecimalNumber),
        "nl" | "letter_number" => matches!(cat, UGC::LetterNumber),
        "no" | "other_number" => matches!(cat, UGC::OtherNumber),
        "p" | "punctuation" => matches!(
            cat,
            UGC::ConnectorPunctuation
                | UGC::DashPunctuation
                | UGC::OpenPunctuation
                | UGC::ClosePunctuation
                | UGC::InitialPunctuation
                | UGC::FinalPunctuation
                | UGC::OtherPunctuation
        ),
        "pc" | "connector_punctuation" => matches!(cat, UGC::ConnectorPunctuation),
        "pd" | "dash_punctuation" => matches!(cat, UGC::DashPunctuation),
        "ps" | "open_punctuation" => matches!(cat, UGC::OpenPunctuation),
        "pe" | "close_punctuation" => matches!(cat, UGC::ClosePunctuation),
        "pi" | "initial_punctuation" => matches!(cat, UGC::InitialPunctuation),
        "pf" | "final_punctuation" => matches!(cat, UGC::FinalPunctuation),
        "po" | "other_punctuation" => matches!(cat, UGC::OtherPunctuation),
        "s" | "symbol" => matches!(
            cat,
            UGC::MathSymbol | UGC::CurrencySymbol | UGC::ModifierSymbol | UGC::OtherSymbol
        ),
        "sm" | "math_symbol" => matches!(cat, UGC::MathSymbol),
        "sc" | "currency_symbol" => matches!(cat, UGC::CurrencySymbol),
        "sk" | "modifier_symbol" => matches!(cat, UGC::ModifierSymbol),
        "so" | "other_symbol" => matches!(cat, UGC::OtherSymbol),
        "z" | "separator" => matches!(
            cat,
            UGC::SpaceSeparator | UGC::LineSeparator | UGC::ParagraphSeparator
        ),
        "zs" | "space_separator" => matches!(cat, UGC::SpaceSeparator),
        "zl" | "line_separator" => matches!(cat, UGC::LineSeparator),
        "zp" | "paragraph_separator" => matches!(cat, UGC::ParagraphSeparator),
        "c" | "control" | "other" => matches!(
            cat,
            UGC::Control | UGC::Format | UGC::PrivateUse | UGC::Unassigned
        ),
        "cc" => matches!(cat, UGC::Control),
        "cf" | "format" => matches!(cat, UGC::Format),
        "co" | "private_use" => matches!(cat, UGC::PrivateUse),
        "cn" | "unassigned" => matches!(cat, UGC::Unassigned),
        _ => false,
    }
}

/// Resolve a script name (after stripping "Is" prefix) to a unicode_script::Script.
fn resolve_script_name(name: &str) -> Option<Script> {
    // Try full name first (e.g. "Latin", "Greek")
    if let Some(s) = Script::from_full_name(name) {
        return Some(s);
    }
    // Try short name (e.g. "Latn", "Grek")
    if let Some(s) = Script::from_short_name(name) {
        return Some(s);
    }
    // Try case-insensitive: capitalize first letter
    let mut capitalized = name.to_lowercase();
    if let Some(first) = capitalized.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Script::from_full_name(&capitalized)
}

/// Java UnicodeBlock.forName is case-insensitive but does not strip arbitrary
/// punctuation. The generated aliases are accepted public-JDK lookup results.
fn resolve_block_name(name: &str) -> Option<u16> {
    crate::java_blocks::resolve(name)
}
