use java_regex::Regex;
#[test]
fn java_single_asymmetry_does_not_change_backrefs() {
    for (pattern, input, expected) in [
        ("(?iu)ß", "ẞ", false),
        ("(?iu)ẞ", "ß", true),
        ("(?iu)[ß]", "ẞ", false),
        ("(?iu)[ẞ]", "ß", true),
        ("(?iu)ß+", "ẞ", false),
        ("(?iu)ẞ+", "ß", true),
        ("(?iu)(ß)\\1", "ßẞ", true),
        ("(?iu)(ẞ)\\1", "ẞß", true),
        ("(?iu)i", "İ", true),
        ("(?iu)ı", "İ", true),
        ("(?iu)ﬀ", "ff", false),
        ("(?i)k(?u)x", "KX", false),
        ("(?i:a)(?iu:b)", "AB", true),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}, {input:?}"
        );
    }
}
#[test]
fn surrogate_class_endpoints_remain_numeric() {
    for (pattern, input, expected) in [
        (r"[a-\x{D800}]", "é", true),
        (r"[a-\uDFFF]", "é", true),
        (r"[\uD800-\uDFFF]", "a", false),
        (r"\uD800", "\0", false),
        (r"[\uD800]", "\0", false),
        (r"[^\uD800]", "a", true),
        (r"[\uD83D\uDE00]", "😀", true),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}"
        );
    }
    assert!(Regex::new(r"[\uD800-b]").is_err());
}
#[test]
fn broader_java_constructs() {
    for (pattern, input, expected) in [
        (r"\X", "e\u{301}", true),
        (r"\p{InBasic_Latin}+", "abc", true),
        (r"(?=a){2}a", "a", true),
        (r"(?<=a{1,2147483647})b", "b", false),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}"
        );
    }
}
#[test]
#[ignore = "private Java reference matrix"]
fn java_reference_matrix() {
    let path = std::env::var("HORIZON_JAVA_REGEX_ORACLE").unwrap();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let pattern = row["pattern"]
            .as_str()
            .unwrap()
            .strip_prefix("[regex]")
            .unwrap_or("");
        if pattern.is_empty() || pattern.contains("\\N{") {
            continue;
        }
        let got = Regex::new(pattern).map(|r| r.matches(row["target"].as_str().unwrap()));
        if let Some(compiled) = row["compiled"].as_bool() {
            assert_eq!(got.is_ok(), compiled, "compile: {}", row);
        }
        assert_eq!(
            got.unwrap_or(false),
            row["matches"].as_bool().unwrap(),
            "{}",
            row
        );
    }
}

#[test]
fn java_25_graphemes_use_unicode_16() {
    assert_eq!(unicode_segmentation::UNICODE_VERSION, (16, 0, 0));
    for (pattern, input, expected) in [
        (r"\X", "\r\n", true),
        (r"\X", "각", true),
        (r"\X", "각", true),
        (r"\X", "क्क", true),
        (r"\X", "؀a", true),
        (r"\X", "का", true),
        (r"\X", "👩́‍💻", true),
        (r"\X", "a‍b", false),
        (r"\X{2}", "🇦🇧🇨", true),
        (r"\X", "🇦🇧🇨", false),
        (r"\X", "\ń", false),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}, {input:?}"
        );
    }
}

#[test]
fn slices_and_grapheme_boundaries_follow_java_nodes() {
    for (pattern, input, expected) in [
        ("(?iu)ßx", "ẞx", true),
        ("(?iu)ßxx", "ẞxx", true),
        ("(?iu)(ß)x", "ẞx", false),
        ("(?iu)ß+x", "ẞx", false),
        (r"\b{g}\X\b{g}", "é", true),
        (r"e\b{g}\u0301", "é", false),
        (r"e\u0301\b{g}b", "éb", true),
        (r"(?iu)[a-z]", "ſ", true),
        (r"(?iu)[i-i]", "ı", true),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}, {input:?}"
        );
    }
}

#[test]
fn character_properties_are_pinned_java_25_facts() {
    assert_eq!(unicode_general_category::UNICODE_VERSION, (16, 0, 0));
    for (pattern, input, expected) in [
        (r"\p{javaLetter}", "ͅ", false),
        (r"\p{javaAlphabetic}", "ͅ", true),
        (r"\p{javaLowerCase}", "ͅ", true),
        (r"\p{javaAlphabetic}", "\u{1e6c0}", false),
        (r"(?U)\w", "\u{1e6c0}", false),
        (r"(?U)\w", "\u{200c}", true),
        (r"(?U)\w", "\u{200d}", true),
        (r"\p{javaWhitespace}", "\u{85}", false),
        (r"\p{javaWhitespace}", "\u{a0}", false),
        (r"\p{javaWhitespace}", "\u{2007}", false),
        (r"\p{javaWhitespace}", "\u{202f}", false),
        (r"\p{javaWhitespace}", "\u{1c}", true),
        (r"\p{javaSpaceChar}", "\u{a0}", true),
        (r"\p{IsWhite_Space}", "\u{85}", true),
        (r"\p{IsWhite_Space}", "\u{1c}", false),
        (r"\p{javaIdentifierIgnorable}", "\u{200c}", true),
        (r"\p{javaUnicodeIdentifierStart}", "\u{200c}", false),
        (r"\p{javaUnicodeIdentifierPart}", "\u{200c}", true),
        (r"\p{javaMirrored}", "<", true),
        (r"\p{javaMirrored}", "/", false),
        (r"\p{IsEmoji}", "😀", true),
        (r"\p{IsExtended_Pictographic}", "😀", true),
        (r"\p{IsControl}", "\u{200c}", false),
    ] {
        assert_eq!(
            Regex::new(pattern).unwrap().matches(input),
            expected,
            "{pattern:?}, {input:?}"
        );
    }
}

#[cfg(feature = "stack-growth")]
#[test]
fn native_stack_growth_preserves_moderately_deep_java_matches() {
    let input = "a".repeat(700);
    for pattern in ["(a){700}", "(a){700}?", "(a){700}+", "a{700}"] {
        assert!(Regex::new(pattern).unwrap().matches(&input), "{pattern}");
    }
    // Force node recursion as well as quantifier recursion.
    let pattern = "(a)".repeat(700);
    assert!(Regex::new(&pattern).unwrap().matches(&input));
}

#[cfg(feature = "stack-growth")]
#[test]
fn native_matching_does_not_silently_stop_at_five_million_steps() {
    // Linear scan, no exponential backtracking. The old counter discards the
    // only match after five million unsuccessful starting positions.
    let mut input = "a".repeat(5_000_010);
    input.push('z');
    let found = Regex::new("z").unwrap().find(&input);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].matched_text, "z");
    assert_eq!(found[0].start, 5_000_010);
}

#[test]
fn java_25_unicode_blocks_and_names_are_exact() {
    for (name, input) in [
        ("Todhri", '\u{105c0}'),
        ("Garay", '\u{10d40}'),
        ("OlOnal", '\u{1e5d0}'),
        ("TULU_TIGALARI", '\u{11380}'),
        ("Tulu-Tigalari", '\u{11380}'),
        ("GurungKhema", '\u{16100}'),
        ("KiratRai", '\u{16d40}'),
        ("Sunuwar", '\u{11bc0}'),
        ("BasicLatin", 'a'),
        ("BASIC_LATIN", 'a'),
        ("Basic Latin", 'a'),
        ("Greek and Coptic", '\u{378}'),
        ("Greek", '\u{378}'),
    ] {
        assert!(
            Regex::new(&format!(r"\p{{In{name}}}"))
                .unwrap()
                .matches(&input.to_string()),
            "{name}"
        );
    }
    for name in [
        "BeriaErfe",
        "TaiYo",
        "Basic-Latin",
        "TuluTigalari",
        "SurrogatesArea",
        "Surrogates Area",
    ] {
        assert!(Regex::new(&format!(r"\p{{In{name}}}")).is_err(), "{name}");
    }
    assert!(Regex::new(r"\p{inBasicLatin}").is_err());
    // The legacy alias is valid but has no UnicodeBlock.of interval.
    let legacy = Regex::new(r"\p{InSURROGATES_AREA}").unwrap();
    assert!(!legacy.matches("a"));
    assert!(!legacy.matches("𐀀"));
    assert!(Regex::new(r"\p{IsTodhri}").unwrap().matches("\u{105c0}"));
    assert!(Regex::new(r"\p{IsBeriaErfe}").is_err());
}

fn reference_name(name: &str) -> Option<u32> {
    match name
        .trim_matches(|c: char| c <= ' ')
        .to_uppercase()
        .as_str()
    {
        "LATIN CAPITAL LETTER A" => Some(0x41),
        "SNOWMAN" => Some(0x2603),
        "HIGH SURROGATES D800" => Some(0xd800),
        "LOW SURROGATES DC00" => Some(0xdc00),
        _ => None,
    }
}

#[test]
fn named_characters_are_lexed_in_context() {
    for (pattern, input, expected) in [
        (r"\N{LATIN CAPITAL LETTER A}", "A", true),
        (r"(?x)\N{LATIN CAPITAL LETTER A}", "A", true),
        (r"\N{ LATIN CAPITAL LETTER A }", "A", true),
        (r"(?x)[ \N{LATIN CAPITAL LETTER A} ]", "A", true),
        (
            r"[\N{LATIN CAPITAL LETTER A}-\N{HIGH SURROGATES D800}]",
            "é",
            true,
        ),
        (
            r"\N{HIGH SURROGATES D800}\N{LOW SURROGATES DC00}",
            "𐀀",
            false,
        ),
        (r"\é", "é", true),
        (r"[\é]", "é", true),
    ] {
        assert_eq!(
            Regex::new_with_name_resolver(pattern, reference_name)
                .unwrap()
                .matches(input),
            expected,
            "{pattern:?}"
        );
    }
    for pattern in [
        r"\N{3}",
        r"\N{}",
        r"\N{SNOWMAN",
        "(?x)\\N{LATIN CAPITAL # c\nLETTER A}",
    ] {
        assert!(
            Regex::new_with_name_resolver(pattern, reference_name).is_err(),
            "{pattern:?}"
        );
    }
    fn must_not_resolve(_: &str) -> Option<u32> {
        panic!("quoted/commented text is not a name escape")
    }
    for (pattern, input) in [
        (r"\Q\N{UNKNOWN}\E", r"\N{UNKNOWN}"),
        (r"(?x)\Q\N{3}\E", r"\N{3}"),
        (r"\\N\{3\}", r"\N{3}"),
        ("(?x)# \\N{3}\nA", "A"),
    ] {
        assert!(Regex::new_with_name_resolver(pattern, must_not_resolve)
            .unwrap()
            .matches(input));
    }
    fn raw(name: &str) -> Option<u32> {
        assert_eq!(name, " LATIN CAPITAL LETTER A ");
        Some(65)
    }
    assert!(
        Regex::new_with_name_resolver(r"(?x)\N{ LATIN CAPITAL LETTER A }", raw)
            .unwrap()
            .matches("A")
    );
}

#[test]
fn reserved_ascii_escape_grammar_matches_java_25() {
    let outside_valid = "123456789AB DGHQRSVWXZabdefhnrstvwz".replace(' ', "");
    let class_valid = "DHSVWadefhnrstvw";
    for byte in (b'0'..=b'9').chain(b'A'..=b'Z').chain(b'a'..=b'z') {
        let c = char::from(byte);
        assert_eq!(
            Regex::new(&format!(r"\{c}")).is_ok(),
            outside_valid.contains(c),
            "outside \\{c}"
        );
        assert_eq!(
            Regex::new(&format!(r"[\{c}]")).is_ok(),
            class_valid.contains(c),
            "class \\{c}"
        );
    }
    assert!(!Regex::new(r"(a)\2").unwrap().matches("aa"));
    assert!(Regex::new(r"\000").unwrap().matches("\0"));
}

#[test]
#[ignore = "private Java lexer compilation/matching matrices"]
fn java_escape_reference_matrix() {
    let path = std::env::var("HORIZON_JAVA_ESCAPE_ORACLE").unwrap();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let re = Regex::new_with_name_resolver(row["pattern"].as_str().unwrap(), reference_name);
        assert_eq!(re.is_ok(), row["compiled"].as_bool().unwrap(), "{}", row);
        if let (Ok(re), Some(input)) = (re, row["target"].as_str()) {
            assert_eq!(
                re.matches(input),
                row["matches"].as_bool().unwrap(),
                "{}",
                row
            );
        }
    }
}

#[test]
fn quantifier_counts_fit_javas_signed_int() {
    // Actual Java 25 compilation/matching oracle: the largest signed int is
    // valid, but larger decimal counts are rejected even when Rust u32 fits.
    assert!(!Regex::new("x{2147483647}").unwrap().matches(""));
    assert!(Regex::new("x{0,2147483647}").unwrap().matches(""));
    for count in [
        "2147483648",
        "4294967295",
        "4294967296",
        "999999999999999999999",
    ] {
        assert!(Regex::new(&format!("x{{{count}}}")).is_err());
        assert!(Regex::new(&format!("x{{0,{count}}}")).is_err());
    }
}
