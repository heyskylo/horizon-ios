// Copyright (c) 2025 Mathieu Dutour Sikiric
// SPDX-License-Identifier: MIT OR Apache-2.0
// Horizon compatibility changes are documented in ../HORIZON.md.

#![no_std]
#![doc = include_str!("../README.md")]

extern crate alloc;

// The `arbitrary::Arbitrary` derive macro (used inside `gen.rs` when the
// `fuzz-gen` feature is on) emits paths like `std::vec::Vec`. Pull in std
// at the crate root so those paths resolve. The rest of the crate stays
// `no_std`; fuzzing consumers (cargo-fuzz, our diff_fuzz binary) all need
// std anyway.
#[cfg(feature = "fuzz-gen")]
extern crate std;

mod engine;
mod java_blocks;
mod java_case;
mod java_properties;
mod parser;
mod types;
mod unicode;

#[doc(hidden)]
pub mod gen;

// ---------------------------------------------------------------------------
// Companion documentation modules. These hold no code — their sole purpose
// is to surface the project's Markdown docs in the rustdoc / docs.rs output,
// so a reader on docs.rs can browse the full reference spec, the documented
// OpenJDK quirks, the intentional differences, and the fuzzing setup without
// leaving the docs site.
// ---------------------------------------------------------------------------

/// Reference specification of OpenJDK's `java.util.regex.Pattern` behavior —
/// the rules this crate reproduces.
#[doc = include_str!("../SPEC.md")]
pub mod spec {}

/// Eight well-known OpenJDK quirks this crate faithfully reproduces, with
/// patterns, expected behavior, and the responsible OpenJDK source class.
#[doc = include_str!("../QUIRKS.md")]
pub mod quirks {}

/// The single intentional deviation from OpenJDK: UTF-16 code-unit offsets
/// vs Unicode-codepoint offsets in `Matcher.start()` / `end()`.
#[doc = include_str!("../DIFFERENCES.md")]
pub mod differences {}

/// How to run the proptest, cargo-fuzz, differential-fuzz, and benchmark
/// suites that protect this crate's byte-for-byte OpenJDK compatibility.
#[doc = include_str!("../FUZZING.md")]
pub mod fuzzing {}

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use engine::{Engine, State};
use parser::Parser;
use types::*;
pub use types::{MatchInfo, PatternSyntaxError};

/// A compiled Java-compatible regular expression.
///
/// Create with [`Regex::new`] or [`Regex::with_flags`], then use
/// [`matches`](Regex::matches), [`find`](Regex::find), [`replace_all`](Regex::replace_all),
/// or [`split`](Regex::split) to apply it.
#[derive(Debug, Clone)]
pub struct Regex {
    source: String,
    pattern: Pattern,
    flags: Flags,
    group_count: usize,
    named_groups: BTreeMap<String, usize>,
}

impl fmt::Display for Regex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl Regex {
    /// Compile a regex pattern with default flags.
    pub fn new(pattern: &str) -> Result<Self, PatternSyntaxError> {
        Self::with_flags(pattern, "")
    }

    /// Compile a regex pattern with the given flags string.
    ///
    /// Supported flag characters:
    /// - `i` — case-insensitive matching
    /// - `m` — multiline mode (`^` and `$` match line boundaries)
    /// - `s` — dotall mode (`.` matches line terminators)
    /// - `x` — comments mode (whitespace and `#` comments ignored)
    /// - `u` — Unicode-aware case folding
    /// - `U` — Unicode character classes for POSIX properties
    /// - `d` — Unix lines mode (only `\n` is a line terminator)
    /// - `l` — literal mode (pattern is treated as a literal string)
    pub fn with_flags(pattern: &str, flags_str: &str) -> Result<Self, PatternSyntaxError> {
        Self::compile(pattern, flags_str, None)
    }

    /// Compile with a caller-supplied Java `Character.codePointOf` name resolver.
    /// Names are read by the lexer, so quoting and comments keep their normal
    /// meaning. The callback receives the raw name, including any spaces.
    /// Surrogate code points are accepted without becoming NUL characters.
    ///
    /// ```
    /// use java_regex::Regex;
    /// fn resolve(name: &str) -> Option<u32> {
    ///     (name == "SNOWMAN").then_some(0x2603)
    /// }
    /// let re = Regex::new_with_name_resolver(r"\N{SNOWMAN}", resolve).unwrap();
    /// assert!(re.matches("☃"));
    /// assert!(Regex::new_with_name_resolver(r"\N{3}", resolve).is_err());
    /// ```
    pub fn new_with_name_resolver(
        pattern: &str,
        resolver: fn(&str) -> Option<u32>,
    ) -> Result<Self, PatternSyntaxError> {
        Self::compile(pattern, "", Some(resolver))
    }

    fn compile(
        pattern: &str,
        flags_str: &str,
        resolver: Option<fn(&str) -> Option<u32>>,
    ) -> Result<Self, PatternSyntaxError> {
        let mut flags = Flags::default();
        for c in flags_str.chars() {
            match c {
                'i' => flags.case_insensitive = true,
                'm' => flags.multiline = true,
                's' => flags.dotall = true,
                'x' => flags.comments = true,
                'U' => flags.unicode_class = true,
                'd' => flags.unix_lines = true,
                'u' => flags.unicode_case = true,
                'l' => flags.literal = true,
                _ => {}
            }
        }

        let parsed_pattern;
        let group_count;
        let named_groups;

        if flags.literal {
            // LITERAL mode: treat the entire pattern as literal text
            let nodes: Vec<Node> = pattern.chars().map(Node::Literal).collect();
            parsed_pattern = Pattern {
                branches: vec![nodes],
            };
            group_count = 0;
            named_groups = BTreeMap::new();
        } else {
            let parser = Parser::new(pattern, flags, resolver);
            let result = parser.parse()?;
            parsed_pattern = result.0;
            group_count = result.1;
            named_groups = result.2;
        }

        Ok(Regex {
            source: pattern.to_string(),
            pattern: parsed_pattern,
            flags,
            group_count,
            named_groups,
        })
    }

    /// Returns the source pattern string.
    pub fn pattern(&self) -> &str {
        &self.source
    }

    /// Returns a literal pattern string that would match the given string.
    ///
    /// Equivalent to Java's `Pattern.quote()`. Wraps the string in `\Q...\E`.
    pub fn quote(s: &str) -> String {
        format!("\\Q{}\\E", s.replace("\\E", "\\E\\\\E\\Q"))
    }

    /// Returns true if the pattern matches the entire input.
    pub fn matches(&self, input: &str) -> bool {
        let input_chars: Vec<char> = input.chars().collect();
        let mut engine = Engine::new(
            &input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        let mut state = State::new(self.group_count);
        let end_anchor = vec![Node::Anchor(AnchorKind::EndOfInput)];
        engine.match_pattern(&self.pattern, &end_anchor, 0, &mut state)
    }

    /// Returns true if the pattern matches the beginning of the input.
    /// Unlike `matches()`, the pattern does not need to match the entire input.
    pub fn looking_at(&self, input: &str) -> Option<MatchInfo> {
        let input_chars: Vec<char> = input.chars().collect();
        let mut engine = Engine::new(
            &input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        if let Some((end_pos, captures)) = engine.try_match_at(&self.pattern, 0) {
            Some(self.build_match_info(&input_chars, 0, end_pos, &captures))
        } else {
            None
        }
    }

    /// Find all non-overlapping matches in the input (eagerly materialized).
    ///
    /// For large inputs prefer [`find_iter`](Regex::find_iter), which yields
    /// matches lazily without allocating a result `Vec` up front.
    pub fn find(&self, input: &str) -> Vec<MatchInfo> {
        self.find_in_region(input, 0, None)
    }

    /// Lazily iterate over all non-overlapping matches in the input.
    ///
    /// Returns a [`Matches`] iterator that yields one [`MatchInfo`] per
    /// `next()` call. The capture-leak state OpenJDK exposes between
    /// successive find attempts (see [`QUIRKS.md`](https://github.com/.../QUIRKS.md))
    /// is preserved across iterator items, so the values match what Java's
    /// `Matcher.find()` loop would produce.
    ///
    /// ```
    /// # use java_regex::Regex;
    /// let re = Regex::new(r"\d+").unwrap();
    /// let positions: Vec<usize> = re.find_iter("a1b22c333").map(|m| m.start).collect();
    /// assert_eq!(positions, vec![1, 3, 6]);
    /// ```
    pub fn find_iter<'r, 'h>(&'r self, input: &'h str) -> Matches<'r, 'h> {
        let input_chars: Vec<char> = input.chars().collect();
        let end = input_chars.len();
        Matches {
            re: self,
            input_chars,
            _haystack: input,
            search_pos: 0,
            prev_match_end: 0,
            end,
            state: State::new(self.group_count),
        }
    }

    /// Find all non-overlapping matches within a region of the input.
    ///
    /// `start` and `end` are char indices. Only text in `[start, end)` is considered.
    /// Anchors like `^` and `$` respect the region boundaries (mirroring Java's
    /// `Matcher.region(start, end)`), but context-dependent lookups still see
    /// the full input — e.g. `\Z`'s "previous char is `\r`" check at the region
    /// start uses the actual prior char, matching OpenJDK's `Dollar` behavior.
    pub fn find_in_region(&self, input: &str, start: usize, end: Option<usize>) -> Vec<MatchInfo> {
        let all_chars: Vec<char> = input.chars().collect();
        let end = end.unwrap_or(all_chars.len()).min(all_chars.len());
        let start = start.min(end);
        self.find_iter_impl_bounded(&all_chars, start, end)
    }

    /// Find the first match starting at or after `start` (char index).
    ///
    /// Returns `None` if no match is found from that position onward.
    pub fn find_at(&self, input: &str, start: usize) -> Option<MatchInfo> {
        let input_chars: Vec<char> = input.chars().collect();
        let input_len = input_chars.len();
        let mut search_pos = start;
        // Persistent State across position attempts — captures from failed
        // attempts leak into the eventually-successful one (matching Java's
        // `Matcher.find(start)` which uses the same Start.match iteration
        // that find() does, with the same per-search groups[] persistence).
        let mut engine = Engine::new(
            &input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        engine.search_start = start;
        let mut state = State::new(self.group_count);
        while search_pos <= input_len {
            if let Some(end_pos) =
                engine.try_match_at_persistent(&self.pattern, search_pos, &mut state)
            {
                return Some(self.build_match_info(
                    &input_chars,
                    search_pos,
                    end_pos,
                    &state.captures,
                ));
            }
            search_pos += 1;
        }
        None
    }

    /// Like `find_iter_impl` but only matches in the half-open range
    /// `[start, end)`. The full input is available to the engine for
    /// context-dependent lookups; bounds only gate where matching starts/ends.
    fn find_iter_impl_bounded(
        &self,
        input_chars: &[char],
        start: usize,
        end: usize,
    ) -> Vec<MatchInfo> {
        let mut results = Vec::new();
        let mut search_pos = start;
        let mut prev_match_end = start;

        let mut engine = Engine::new(
            input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        engine.text_start = start;
        engine.text_end = end;
        let mut state = State::new(self.group_count);

        while search_pos <= end {
            engine.search_start = prev_match_end;

            if let Some(end_pos) =
                engine.try_match_at_persistent(&self.pattern, search_pos, &mut state)
            {
                results.push(self.build_match_info(
                    input_chars,
                    search_pos,
                    end_pos,
                    &state.captures,
                ));

                prev_match_end = end_pos;
                if end_pos == search_pos {
                    search_pos += 1;
                } else {
                    search_pos = end_pos;
                }
                state = State::new(self.group_count);
            } else {
                search_pos += 1;
            }
        }

        results
    }

    fn build_match_info(
        &self,
        input_chars: &[char],
        start: usize,
        end: usize,
        captures: &[Option<(usize, usize)>],
    ) -> MatchInfo {
        let matched_text: String = input_chars[start..end].iter().collect();

        let mut groups = Vec::new();
        let mut group_positions = Vec::new();
        for i in 1..=self.group_count {
            if let Some(Some((s, e))) = captures.get(i) {
                groups.push(Some(input_chars[*s..*e].iter().collect::<String>()));
                group_positions.push(Some((*s, *e)));
            } else {
                groups.push(None);
                group_positions.push(None);
            }
        }

        let mut named = BTreeMap::new();
        for (name, &idx) in &self.named_groups {
            if let Some(Some((s, e))) = captures.get(idx) {
                named.insert(name.clone(), input_chars[*s..*e].iter().collect::<String>());
            }
        }

        MatchInfo {
            matched_text,
            start,
            end,
            groups,
            group_positions,
            named_groups: named,
        }
    }

    /// Replace all matches. Accepts either a replacement-DSL string (with
    /// Java's `$1`/`${name}`/`\$` syntax) or a closure that receives each
    /// `MatchInfo` and returns the replacement.
    ///
    /// ```
    /// # use java_regex::Regex;
    /// let re = Regex::new(r"(\d+)").unwrap();
    /// assert_eq!(re.replace_all("a1b22", "($1)"), "a(1)b(22)");
    /// assert_eq!(re.replace_all("a1b22", |m: &java_regex::MatchInfo| {
    ///     format!("<{}>", m.matched_text.len())
    /// }), "a<1>b<2>");
    /// ```
    pub fn replace_all<R: Replacer>(&self, input: &str, replacer: R) -> String {
        let input_chars: Vec<char> = input.chars().collect();
        self.replace_internal(&input_chars, replacer, false)
    }

    /// Replace the first match only. Same accept-any-Replacer semantics as
    /// [`replace_all`](Regex::replace_all).
    pub fn replace_first<R: Replacer>(&self, input: &str, replacer: R) -> String {
        let input_chars: Vec<char> = input.chars().collect();
        self.replace_internal(&input_chars, replacer, true)
    }

    fn replace_internal<R: Replacer>(
        &self,
        input_chars: &[char],
        mut replacer: R,
        first_only: bool,
    ) -> String {
        let input_len = input_chars.len();
        let mut result = String::new();
        let mut last_end = 0;
        let mut search_pos = 0;

        // Persistent State so captures from failed find attempts leak into
        // the eventual successful match, matching Java's `appendReplacement`
        // which uses the same find() semantics. State is reset after each
        // successful replacement (Java's per-search groups[] reset).
        let mut engine = Engine::new(
            input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        let mut state = State::new(self.group_count);

        let mut prev_match_end = 0;
        while search_pos <= input_len {
            engine.search_start = prev_match_end;

            if let Some(end_pos) =
                engine.try_match_at_persistent(&self.pattern, search_pos, &mut state)
            {
                result.extend(&input_chars[last_end..search_pos]);

                if !state.captures.is_empty() {
                    state.captures[0] = Some((search_pos, end_pos));
                }

                let info = self.build_match_info(input_chars, search_pos, end_pos, &state.captures);
                replacer.replace_append(&info, &mut result);

                last_end = end_pos;
                prev_match_end = end_pos;
                if end_pos == search_pos {
                    if search_pos < input_len {
                        result.push(input_chars[search_pos]);
                        last_end = search_pos + 1;
                    }
                    search_pos += 1;
                } else {
                    search_pos = end_pos;
                }

                state = State::new(self.group_count);
                if first_only {
                    break;
                }
            } else {
                search_pos += 1;
            }
        }

        result.extend(&input_chars[last_end..]);
        result
    }
}

// ---------------------------------------------------------------------------
// Matches iterator
// ---------------------------------------------------------------------------

/// Lazy iterator over non-overlapping matches, returned by
/// [`Regex::find_iter`]. Yields one [`MatchInfo`] per `next()` call.
///
/// Holds its own char-indexed copy of the input plus a persistent matcher
/// `State` so capture-leak semantics across successive `next()` calls match
/// what an equivalent `Matcher.find()` loop in Java would produce.
pub struct Matches<'r, 'h> {
    re: &'r Regex,
    input_chars: Vec<char>,
    _haystack: &'h str,
    search_pos: usize,
    prev_match_end: usize,
    end: usize,
    state: State,
}

impl<'r, 'h> Iterator for Matches<'r, 'h> {
    type Item = MatchInfo;

    fn next(&mut self) -> Option<MatchInfo> {
        while self.search_pos <= self.end {
            let mut engine = Engine::new(
                &self.input_chars,
                self.re.flags,
                self.re.group_count,
                &self.re.named_groups,
            );
            engine.text_start = 0;
            engine.text_end = self.end;
            engine.search_start = self.prev_match_end;

            if let Some(end_pos) =
                engine.try_match_at_persistent(&self.re.pattern, self.search_pos, &mut self.state)
            {
                let m = self.re.build_match_info(
                    &self.input_chars,
                    self.search_pos,
                    end_pos,
                    &self.state.captures,
                );
                self.prev_match_end = end_pos;
                if end_pos == self.search_pos {
                    self.search_pos += 1;
                } else {
                    self.search_pos = end_pos;
                }
                self.state = State::new(self.re.group_count);
                return Some(m);
            } else {
                self.search_pos += 1;
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Replacer trait
// ---------------------------------------------------------------------------

/// A source of replacement text for [`Regex::replace_all`] / [`Regex::replace_first`].
///
/// Implementations exist for:
/// - `&str` / `String` / `&String` — treated as Java's replacement DSL
///   (`$1`, `${name}`, `\$`, `\\`, etc.).
/// - any `FnMut(&MatchInfo) -> String` — called once per match.
///
/// User code can implement `Replacer` for custom types if neither variant fits.
pub trait Replacer {
    /// Append the replacement for `m` to `dst`.
    fn replace_append(&mut self, m: &MatchInfo, dst: &mut String);
}

/// Apply Java's `Matcher.appendReplacement` DSL: `$N`, `${name}`, `\$`, `\\`.
fn expand_java_replacement(replacement: &str, m: &MatchInfo, dst: &mut String) {
    let rep_chars: Vec<char> = replacement.chars().collect();
    let mut i = 0;
    while i < rep_chars.len() {
        if rep_chars[i] == '\\' && i + 1 < rep_chars.len() {
            dst.push(rep_chars[i + 1]);
            i += 2;
        } else if rep_chars[i] == '$' {
            i += 1;
            if i < rep_chars.len() && rep_chars[i] == '{' {
                i += 1;
                let mut name = String::new();
                while i < rep_chars.len() && rep_chars[i] != '}' {
                    name.push(rep_chars[i]);
                    i += 1;
                }
                if i < rep_chars.len() {
                    i += 1;
                }
                if let Some(val) = m.named_groups.get(&name) {
                    dst.push_str(val);
                }
            } else if i < rep_chars.len() && rep_chars[i].is_ascii_digit() {
                let mut num = (rep_chars[i] as u32 - '0' as u32) as usize;
                i += 1;
                while i < rep_chars.len() && rep_chars[i].is_ascii_digit() {
                    let new_num = num * 10 + (rep_chars[i] as u32 - '0' as u32) as usize;
                    if new_num <= m.groups.len() {
                        num = new_num;
                        i += 1;
                    } else {
                        break;
                    }
                }
                // group 0 = whole match; groups[i-1] = capture group i.
                if num == 0 {
                    dst.push_str(&m.matched_text);
                } else if let Some(Some(g)) = m.groups.get(num - 1) {
                    dst.push_str(g);
                }
            } else {
                dst.push('$');
            }
        } else {
            dst.push(rep_chars[i]);
            i += 1;
        }
    }
}

impl Replacer for &str {
    fn replace_append(&mut self, m: &MatchInfo, dst: &mut String) {
        expand_java_replacement(self, m, dst);
    }
}

impl Replacer for String {
    fn replace_append(&mut self, m: &MatchInfo, dst: &mut String) {
        expand_java_replacement(self.as_str(), m, dst);
    }
}

impl Replacer for &String {
    fn replace_append(&mut self, m: &MatchInfo, dst: &mut String) {
        expand_java_replacement(self.as_str(), m, dst);
    }
}

impl<F> Replacer for F
where
    F: FnMut(&MatchInfo) -> String,
{
    fn replace_append(&mut self, m: &MatchInfo, dst: &mut String) {
        dst.push_str(&self(m));
    }
}

impl Regex {
    /// Split the input by pattern matches (Java String.split semantics, limit=0).
    pub fn split(&self, input: &str) -> Vec<String> {
        self.split_with_limit(input, 0)
    }

    /// Split the input by pattern matches with a limit parameter.
    /// - limit > 0: at most `limit` parts, last part contains the remainder
    /// - limit == 0: no limit, trailing empty strings removed (Java default)
    /// - limit < 0: no limit, trailing empty strings preserved
    pub fn split_with_limit(&self, input: &str, limit: i32) -> Vec<String> {
        let input_chars: Vec<char> = input.chars().collect();
        let input_len = input_chars.len();
        let mut parts: Vec<String> = Vec::new();
        let mut index = 0;
        let mut search_pos = 0;

        // Persistent State across position attempts — relevant when the
        // pattern uses backreferences that the capture leak across positions
        // could change the result of (rare for split, but matches Java).
        let mut engine = Engine::new(
            &input_chars,
            self.flags,
            self.group_count,
            &self.named_groups,
        );
        let mut state = State::new(self.group_count);

        let mut prev_match_end = 0;
        while search_pos <= input_len {
            if limit > 0 && parts.len() as i32 >= limit - 1 {
                break;
            }

            engine.search_start = prev_match_end;

            if let Some(end_pos) =
                engine.try_match_at_persistent(&self.pattern, search_pos, &mut state)
            {
                // Java quirk: a zero-width match at position 0 produces NO leading
                // empty substring. OpenJDK's Pattern.split has the explicit check:
                //     if (index == 0 && index == m.start() && m.start() == m.end()) continue;
                if index == 0 && search_pos == 0 && end_pos == 0 {
                    prev_match_end = end_pos;
                    search_pos = 1;
                    state = State::new(self.group_count);
                    continue;
                }
                parts.push(input_chars[index..search_pos].iter().collect());
                index = end_pos;
                prev_match_end = end_pos;
                if end_pos == search_pos {
                    search_pos += 1;
                } else {
                    search_pos = end_pos;
                }
                state = State::new(self.group_count);
            } else {
                search_pos += 1;
            }
        }

        parts.push(input_chars[index..].iter().collect());

        // Java limit=0: remove trailing empty strings
        if limit == 0 {
            while parts.last().is_some_and(|s| s.is_empty()) {
                parts.pop();
            }
        }

        if parts.is_empty() && index == 0 {
            parts.push(input.to_string());
        }

        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_match() {
        let regex = Regex::new("abc").unwrap();
        assert!(regex.matches("abc"));
        assert!(!regex.matches("zabc"));
    }

    #[test]
    fn test_find() {
        let regex = Regex::new("abc").unwrap();
        let matches = regex.find("zabc");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "abc");
    }

    #[test]
    fn test_escaped_dot() {
        let regex = Regex::new("\\.").unwrap();
        assert!(regex.matches("."));
        assert!(!regex.matches("a"));
    }

    #[test]
    fn test_escaped_backslash() {
        let regex = Regex::new("\\\\").unwrap();
        assert!(regex.matches("\\"));
    }

    #[test]
    fn test_quantifiers() {
        let regex = Regex::new("a{3}").unwrap();
        let matches = regex.find("aaab");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "aaa");
    }

    #[test]
    fn test_groups() {
        let regex = Regex::new("(a)(b)(c)").unwrap();
        let matches = regex.find("abc");
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].groups,
            vec![
                Some("a".to_string()),
                Some("b".to_string()),
                Some("c".to_string())
            ]
        );
    }

    #[test]
    fn test_alternative() {
        let regex = Regex::new("cat|dog").unwrap();
        assert!(regex.matches("dog"));
        assert!(regex.matches("cat"));
        assert!(!regex.matches("bird"));
    }

    #[test]
    fn test_word_boundary() {
        let regex = Regex::new("\\bcat\\b").unwrap();
        let matches = regex.find("a cat! bobcat cat_");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "cat");
    }

    #[test]
    fn test_multiline() {
        let regex = Regex::with_flags("^abc", "m").unwrap();
        let matches = regex.find("abc\nabc");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_dotall() {
        let regex = Regex::with_flags("a.*b", "s").unwrap();
        assert!(regex.matches("a\nb"));
    }

    #[test]
    fn test_quoted() {
        let regex = Regex::new("\\Q.*+?\\E").unwrap();
        assert!(regex.matches(".*+?"));
        assert!(!regex.matches("abc"));
    }

    #[test]
    fn test_char_class() {
        let regex = Regex::new("[abc]+").unwrap();
        assert!(regex.matches("abcba"));
        assert!(!regex.matches("def"));
    }

    #[test]
    fn test_negated_char_class() {
        let regex = Regex::new("[^abc]+").unwrap();
        assert!(regex.matches("def"));
        assert!(!regex.matches("abc"));
    }

    #[test]
    fn test_char_class_range() {
        let regex = Regex::new("[a-z]+").unwrap();
        assert!(regex.matches("hello"));
        assert!(!regex.matches("HELLO"));
    }

    #[test]
    fn test_char_class_intersection() {
        let regex = Regex::new("[a-z&&[^aeiou]]+").unwrap();
        assert!(regex.matches("bcdfg"));
        assert!(!regex.matches("aei"));
    }

    #[test]
    fn test_predefined_classes() {
        assert!(Regex::new("\\d+").unwrap().matches("123"));
        assert!(Regex::new("\\w+").unwrap().matches("hello_123"));
        assert!(Regex::new("\\s+").unwrap().matches("  \t"));
    }

    #[test]
    fn test_backreference() {
        let regex = Regex::new("(\\w+)\\s+\\1").unwrap();
        assert!(regex.matches("hello hello"));
        assert!(!regex.matches("hello world"));
    }

    #[test]
    fn test_lookahead() {
        let regex = Regex::new("\\w+(?=:)").unwrap();
        let matches = regex.find("key:value");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "key");
    }

    #[test]
    fn test_lookbehind() {
        let regex = Regex::new("(?<=:)\\w+").unwrap();
        let matches = regex.find("key:value");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "value");
    }

    #[test]
    fn test_atomic_group() {
        let regex = Regex::new("(?>a|ab)c").unwrap();
        assert!(!regex.matches("abc"));
    }

    #[test]
    fn test_case_insensitive() {
        let regex = Regex::with_flags("abc", "i").unwrap();
        assert!(regex.matches("ABC"));
        assert!(regex.matches("aBc"));
    }

    #[test]
    fn test_possessive() {
        let regex = Regex::new("a.*+b").unwrap();
        assert!(!regex.matches("a123b"));
    }

    #[test]
    fn test_reluctant() {
        let regex = Regex::new("<.*?>").unwrap();
        let matches = regex.find("<b>bold</b>");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "<b>");
        assert_eq!(matches[1].matched_text, "</b>");
    }

    #[test]
    fn test_replace_all() {
        let regex = Regex::new("\\d+").unwrap();
        assert_eq!(regex.replace_all("a1b22c333", "#"), "a#b#c#");
    }

    #[test]
    fn test_replace_with_groups() {
        let regex = Regex::new("(\\w+),(\\w+)").unwrap();
        assert_eq!(regex.replace_all("Doe,John", "$2 $1"), "John Doe");
    }

    #[test]
    fn test_split() {
        let regex = Regex::new("\\s*,\\s*").unwrap();
        assert_eq!(regex.split("a, b,  c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_compile_error_duplicate_name() {
        assert!(Regex::new("(?<x>a)(?<x>b)").is_err());
    }

    #[test]
    fn test_compile_error_bad_range() {
        assert!(Regex::new("[z-a]").is_err());
    }

    #[test]
    fn test_hex_escape() {
        let regex = Regex::new("\\x41+").unwrap();
        assert!(regex.matches("AAA"));
    }

    #[test]
    fn test_unicode_escape() {
        let regex = Regex::new("\\u0041+").unwrap();
        assert!(regex.matches("AAA"));
    }

    #[test]
    fn test_empty_match() {
        let regex = Regex::new(".*").unwrap();
        assert!(regex.matches(""));
        assert!(regex.matches("anything"));
    }

    #[test]
    fn test_group_quantifier() {
        let regex = Regex::new("(ab){2,3}").unwrap();
        assert!(regex.matches("abab"));
        assert!(regex.matches("ababab"));
        assert!(!regex.matches("ab"));
    }

    #[test]
    fn test_comments_mode() {
        let regex = Regex::new("(?x)a \\s+ b # comment").unwrap();
        assert!(regex.matches("a   b"));
    }

    #[test]
    fn test_linebreak() {
        let regex = Regex::new("\\R").unwrap();
        let matches = regex.find("a\r\nb\nc\rd");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].matched_text, "\r\n");
        assert_eq!(matches[1].matched_text, "\n");
        assert_eq!(matches[2].matched_text, "\r");
    }

    #[test]
    fn test_named_groups() {
        let regex = Regex::new("(?<last>\\w+),(?<first>\\w+)").unwrap();
        let result = regex.replace_all("Doe,John", "${first} ${last}");
        assert_eq!(result, "John Doe");
    }

    #[test]
    fn test_octal() {
        let regex = Regex::new("\\012").unwrap();
        assert!(regex.matches("\n"));
    }

    #[test]
    fn test_control_escape() {
        let regex = Regex::new("\\cJ").unwrap();
        assert!(regex.matches("\n"));
    }

    #[test]
    fn test_nested_char_class_with_colon() {
        let regex = Regex::new("[[:digit:]]+").unwrap();
        assert!(regex.matches("dig:t"));
        assert!(!regex.matches("023"));
    }

    #[test]
    fn test_anchors() {
        let regex = Regex::new("\\Aabc").unwrap();
        let matches = regex.find("abc\nxabc");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "abc");
    }

    #[test]
    fn test_inline_flags() {
        let regex = Regex::new("(?i)abc").unwrap();
        assert!(regex.matches("ABC"));
    }

    #[test]
    fn test_non_capturing_group() {
        let regex = Regex::new("(?:ab)+c").unwrap();
        assert!(regex.matches("ababc"));
        let matches = regex.find("ababc");
        assert_eq!(matches[0].groups.len(), 0);
    }

    #[test]
    fn test_star_empty() {
        let regex = Regex::new("a*").unwrap();
        assert!(regex.matches(""));
        assert!(regex.matches("aaa"));
    }

    #[test]
    fn test_negative_lookahead() {
        let regex = Regex::new("foo(?!bar)").unwrap();
        let matches = regex.find("foobaz foobar");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "foo");
    }

    #[test]
    fn test_backtracking_alternation() {
        let regex = Regex::new("(a|ab)c").unwrap();
        assert!(regex.matches("abc"));
    }

    #[test]
    fn test_unicode_property() {
        let regex = Regex::new("\\p{L}+").unwrap();
        let matches = regex.find("Grüße");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "Grüße");
    }

    #[test]
    fn test_variable_lookbehind() {
        let regex = Regex::new("(?<=\\w+)\\d+").unwrap();
        let matches = regex.find("a1");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_text, "1");
    }

    #[test]
    fn test_looking_at() {
        let regex = Regex::new("\\d+").unwrap();
        let m = regex.looking_at("123abc");
        assert!(m.is_some());
        assert_eq!(m.unwrap().matched_text, "123");

        assert!(regex.looking_at("abc123").is_none());
    }

    #[test]
    fn test_replace_first() {
        let regex = Regex::new("\\d+").unwrap();
        assert_eq!(regex.replace_first("a1b2c3", "#"), "a#b2c3");
    }

    #[test]
    fn test_replace_first_with_groups() {
        let regex = Regex::new("(\\w+),(\\w+)").unwrap();
        assert_eq!(regex.replace_first("a,b and c,d", "$2,$1"), "b,a and c,d");
    }

    #[test]
    fn test_split_with_limit() {
        let regex = Regex::new(",").unwrap();
        assert_eq!(regex.split_with_limit("a,b,c,d", 2), vec!["a", "b,c,d"]);
        assert_eq!(regex.split_with_limit("a,b,c,d", 3), vec!["a", "b", "c,d"]);
        assert_eq!(
            regex.split_with_limit("a,b,c,d", -1),
            vec!["a", "b", "c", "d"]
        );
    }

    #[test]
    fn test_split_with_limit_trailing_empty() {
        let regex = Regex::new(",").unwrap();
        // limit=0 (default): trailing empties removed
        assert_eq!(regex.split("a,,b,"), vec!["a", "", "b"]);
        // limit=-1: trailing empties preserved
        assert_eq!(regex.split_with_limit("a,,b,", -1), vec!["a", "", "b", ""]);
    }

    #[test]
    fn test_group_positions() {
        let regex = Regex::new("(\\w+)@(\\w+)").unwrap();
        let matches = regex.find("user@host");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].group_positions, vec![Some((0, 4)), Some((5, 9))]);
    }

    #[test]
    #[cfg(not(feature = "stack-growth"))]
    fn test_backtracking_limit() {
        // Catastrophic backtracking pattern — should not hang
        let regex = Regex::new("(a+)+b").unwrap();
        // This would take exponential time without step limits
        assert!(!regex.matches("aaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn test_literal_flag() {
        let regex = Regex::with_flags("a.b+c", "l").unwrap();
        assert!(regex.matches("a.b+c"));
        assert!(!regex.matches("axbc"));
    }

    #[test]
    fn test_quote() {
        let quoted = Regex::quote("hello.world*");
        let regex = Regex::new(&quoted).unwrap();
        assert!(regex.matches("hello.world*"));
        assert!(!regex.matches("helloXworld"));
    }

    #[test]
    fn test_find_at() {
        let regex = Regex::new("\\d+").unwrap();
        let m = regex.find_at("a1b22c333", 3);
        assert!(m.is_some());
        assert_eq!(m.unwrap().matched_text, "22");
    }

    #[test]
    fn test_find_in_region() {
        let regex = Regex::new("\\d+").unwrap();
        let matches = regex.find_in_region("a1b22c333d", 2, Some(9));
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "22");
        assert_eq!(matches[0].start, 3);
        assert_eq!(matches[1].matched_text, "333");
        assert_eq!(matches[1].start, 6);
    }

    #[test]
    fn test_replace_all_closure() {
        // After the Replacer trait refactor, closures impl Replacer directly,
        // so the old `replace_all_with` collapses into `replace_all`.
        let regex = Regex::new("\\d+").unwrap();
        let result = regex.replace_all("a1b22c333", |m: &MatchInfo| {
            format!("[{}]", m.matched_text.len())
        });
        assert_eq!(result, "a[1]b[2]c[3]");
    }

    #[test]
    fn test_replace_first_closure() {
        let regex = Regex::new("\\w+").unwrap();
        let result =
            regex.replace_first("hello world", |m: &MatchInfo| m.matched_text.to_uppercase());
        assert_eq!(result, "HELLO world");
    }

    #[test]
    fn test_display() {
        let regex = Regex::new("\\d+").unwrap();
        assert_eq!(format!("{}", regex), "\\d+");
    }

    #[test]
    fn test_pattern() {
        let regex = Regex::new("(?i)abc").unwrap();
        assert_eq!(regex.pattern(), "(?i)abc");
    }

    // -- regressions found by examples/diff_fuzz.rs against OpenJDK 25 -------

    #[test]
    fn test_split_zero_width_at_start_no_leading_empty() {
        // OpenJDK Pattern.split skips a zero-width match at position 0.
        let r = Regex::with_flags(r"\Q\E", "i").unwrap();
        assert_eq!(r.split("\t"), vec!["\t"]);
        let r = Regex::new("(?:)").unwrap();
        assert_eq!(r.split("\r"), vec!["\r"]);
        // Zero-width matches at EVERY position split into individual chars
        // (because the leading empty at pos 0 is suppressed, but the internal
        // ones are not — the input is sliced between every char).
        let r = Regex::new(r"\Q\E").unwrap();
        assert_eq!(r.split("abc"), vec!["a", "b", "c"]);
        // Lookahead-only zero-width matches are also splits — same suppression.
        let r = Regex::new(r"(?=b)").unwrap();
        assert_eq!(r.split("abc"), vec!["a", "bc"]);
        // Empty input: split returns a single empty element (no suppression
        // because there's only one position and it gets emitted as the tail).
        let r = Regex::new(r"\Q\E").unwrap();
        assert_eq!(r.split(""), vec![""]);
        // Sanity: non-zero-width match at pos 0 does NOT suppress the leading
        // empty (the suppression is specifically zero-width).
        let r = Regex::new("a").unwrap();
        assert_eq!(r.split("abc"), vec!["", "bc"]);
        // Zero-width lookbehind anchor at pos 0 only — no further matches.
        let r = Regex::new(r"(?<=^)").unwrap();
        assert_eq!(r.split("abc"), vec!["abc"]);
    }

    #[test]
    fn test_multiline_caret_not_at_end_of_input() {
        // Java's multiline ^ never matches at end of input (Perl/Java quirk).
        let r = Regex::with_flags("^", "m").unwrap();
        assert!(!r.matches(""));
        let r = Regex::with_flags("^", "m").unwrap();
        let ms = r.find("\r\ré\n");
        assert_eq!(ms.len(), 3, "expected matches at positions 0, 1, 2 only");
        assert_eq!(
            ms.iter().map(|m| m.start).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn test_quantified_deterministic_atom_is_atomic() {
        // OpenJDK's Curly/GroupCurly does not backtrack into a quantified atom
        // when the atom's body is "deterministic" (no top-level alternation,
        // no variable-count nested quantifier). So `\R{2}` on "\r\n" does NOT
        // match — iter 1 takes the longer \r\n branch, iter 2 has nothing left,
        // and the engine doesn't retry iter 1 with the shorter \r-only branch.
        // The same applies when \R is wrapped in any kind of non-alternation
        // group, including non-capturing, capturing, atomic, and flag groups.
        for pat in &[
            r"\R{2}",
            r"(?:\R){2}",
            r"(\R){2}",
            r"(?>\R){2}",
            r"(?i:\R){2}",
            r"(?im:\R){2}",
        ] {
            assert!(
                !Regex::new(pat).unwrap().matches("\r\n"),
                "expected no match for {pat:?}"
            );
        }
    }

    #[test]
    fn test_quantified_non_deterministic_atom_backtracks() {
        // Multi-branch (alternation) bodies stay non-atomic — Loop in OpenJDK,
        // continuation-based backtracking here.
        assert!(Regex::new(r"(?:a|aa){2}").unwrap().matches("aaa"));
        assert!(Regex::new(r"(a|aa){2}").unwrap().matches("aaaa"));
        assert!(Regex::new(r"(?:ab|a){2}").unwrap().matches("aab"));
        // Mix: alternation between deterministic atoms (one of which is \R).
        assert!(Regex::new(r"(?:a|\R){2}").unwrap().matches("a\r\n"));
    }

    #[test]
    fn test_linebreak_is_not_atomic() {
        // \R is documented as `\r\n|[line-break-chars]` (a regular alternation,
        // not an atomic group). Java accepts `\R\n` matching "\r\n" because the
        // first \R backtracks from `\r\n` to just `\r`, letting the trailing
        // `\n` succeed. The previous implementation was atomic and rejected.
        let r = Regex::new(r"\R\n").unwrap();
        assert!(r.matches("\r\n"));
        // (?<!\R) at position 1 of "\r\n" should fail: \r alone is a \R.
        let r = Regex::new(r"(?<!\R)").unwrap();
        let ms = r.find("\r\n");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].start, 0);
    }

    #[test]
    fn test_zero_max_body_with_star_quantifier_leaves_group_unset() {
        // OpenJDK quirk: when a Group is quantified with cmin=0 and the body
        // has max-length 0 (truly empty, or only zero-width content like
        // \Q\E / anchors / lookarounds), the group is treated as not having
        // executed — `g_i` stays null. Mirrors GroupCurly's `locals[i] = -1`
        // sentinel that tells GroupTail to skip the capture write.
        let cases = [
            (r"(\Q\E)*", true), // empty body + * → g1 should be null
            (r"()*", true),     // truly empty body + *
            (r"(\Q\E)", false), // no quantifier → g1 should be ""
            (r"()", false),     // no quantifier
            (r"()+", false),    // + (cmin=1) → body actually runs once → g1=""
        ];
        for (pat, expect_null) in cases {
            let r = Regex::new(pat).unwrap();
            let m = &r.find("ab")[0];
            let got = m.groups.first().cloned().flatten();
            if expect_null {
                assert_eq!(got, None, "{pat}: expected null capture, got {got:?}");
            } else {
                assert_eq!(
                    got.as_deref(),
                    Some(""),
                    "{pat}: expected empty-string capture, got {got:?}"
                );
            }
        }
        // And: a group whose body CAN match (e.g. `a*` or `a?`) still gets ""
        // even with `*` — the body did run, just matched zero chars by choice.
        assert_eq!(
            Regex::new(r"(a*)*").unwrap().find("ab")[0]
                .groups
                .first()
                .cloned()
                .flatten()
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn test_end_anchor_with_region_bounds_sees_full_input() {
        // `\Z` at position 1 of region [1,2) of "\r\n\r" must see the previous
        // `\r` (outside the region) to recognize it's mid-`\r\n` and NOT match.
        // OpenJDK's `Dollar.match` does `seq.charAt(i-1)` without honoring
        // region bounds; we mirror by passing the full input + bounds to the
        // engine rather than slicing the input down to the region.
        let re = Regex::with_flags("\\Z", "ms").unwrap();
        let matches = re.find_in_region("\r\n\r", 1, Some(2));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 2);
    }

    #[test]
    fn test_capture_leak_across_find_positions() {
        // Java's Matcher keeps groups[] across the position-iteration loop in
        // Start.match — captures set during a failed find attempt at position N
        // leak into the successful match found at a later position. Mirrored
        // here via try_match_at_persistent + no save/restore in the quantifier
        // helpers.
        let re = Regex::new(r"(?=(\w))*\s").unwrap();
        let matches = re.find("a ");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 1);
        assert_eq!(matches[0].end, 2);
        assert_eq!(
            matches[0].groups.first().cloned().flatten(),
            Some("a".to_string()),
            "group 1 should leak from failed pos 0 attempt"
        );
    }

    #[test]
    fn test_capture_leak_from_negative_lookbehind() {
        // Java's NotBehind doesn't save/restore groups[] around the inner
        // match — when the inner matches (and the negative inversion makes
        // the overall lookbehind fail), the inner's captures persist.
        let re = Regex::new(r"(?<!(a|bb))c?").unwrap();
        let matches = re.find("ac");
        assert_eq!(matches.len(), 2);
        // First match at pos 0: lookbehind succeeded (no chars before pos 0
        // to match (a|bb)), no inner capture happened.
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].groups.first().cloned().flatten(), None);
        // Second match at pos 2: lookbehind succeeded *here*, but the failed
        // pos 1 attempt's inner (a|bb) had matched `a` at pos 0, leaking g1.
        assert_eq!(matches[1].start, 2);
        assert_eq!(
            matches[1].groups.first().cloned().flatten(),
            Some("a".to_string()),
            "group 1 should leak from failed pos 1 lookbehind attempt"
        );
    }

    #[test]
    fn test_zero_width_alt_clears_failed_capture() {
        // `(?:(a)|(?=.)){2,3}?` on "ba" at pos 1: outer quantifier matches via
        // alt2 (?=.) zero-width twice. Java's LazyLoop bails at i==beginIndex
        // so alt1=(a) never re-fires for iters 2+. g1 should be null.
        // Previously alt1 captured "a" in iter 1's attempt and leaked into the
        // final zero-width match.
        let re = Regex::new(r"(?:(a)|(?=.)){2,3}?").unwrap();
        let matches = re.find("ba");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[1].start, 1);
        assert_eq!(matches[1].end, 1);
        assert_eq!(
            matches[1].groups.first().cloned().flatten(),
            None,
            "g1 must be null when outer succeeds via the zero-width alt"
        );
    }

    #[test]
    fn test_neg_lookahead_with_noncap_quant_capture_persists() {
        // `(?!(?:(X)){2,}?)` on input where (X) matches once but cmin=2 can't be
        // reached. Java's inner LazyLoop sets g1 from iter 1's GroupTail then
        // returns false; the negative-lookahead inversion makes the outer match
        // succeed and the inner capture LEAKS into the final state.
        let re = Regex::new(r"(?!(?:(\n)){2,}?)").unwrap();
        let matches = re.find("h\n>");
        // At pos 1, inner attempts: \n matches at pos 1 (count=1), then \n at
        // pos 2 fails (count=2 not reached). Inner fails. Neg lookahead succeeds.
        // g1 should leak as "\n".
        assert_eq!(matches.len(), 4);
        assert_eq!(matches[1].start, 1);
        assert_eq!(matches[1].end, 1);
        assert_eq!(
            matches[1].groups.first().cloned().flatten(),
            Some("\n".to_string()),
            "g1 should leak from the failed iter-2 attempt at pos 1"
        );
    }

    #[test]
    fn test_nondet_reluctant_zero_width_body_rejects() {
        // `(?:((\1[^\w])*?)){2,3}?` on "\t" must NOT match: outer reluctant
        // {2,3}? min=2 with a non-deterministic zero-width body. Java's Prolog
        // calls body.match once; the body's reluctant `*?` returns zero-width
        // via 0 inner iters; then chain unwinds to LazyLoop which bails at
        // i==beginIndex. Body never advances → matches=false.
        //
        // Previously our impl leaked group-1's capture from Path 1's
        // atomic body match into Path 2's continuation, where the inner `\1`
        // could succeed against the leaked empty capture and consume `\t`.
        // The fix restores `state.captures = saved.clone()` when Path 1's
        // zero-width body fails the cmin requirement on non-deterministic
        // bodies, mirroring Java's chain-unwind GroupTail restoration.
        let re = Regex::new(r"(?:((\1[^\w])*?)){2,3}?").unwrap();
        assert!(
            !re.matches("\t"),
            "outer reluctant {{2,3}}? with non-det zero-width body must not match"
        );
    }

    #[test]
    fn test_lookbehind_unbounded_body_overflow_threshold() {
        // Java's `Pattern.java::TreeInfo` computes lookbehind `info.maxLength`
        // using i32 wrapping arithmetic. A possessive `*+` contributes
        // MAX_REPS; a bounded sibling/prefix then pushes maxLength past
        // Integer.MAX_VALUE, causing signed overflow. In `NotBehind.match`,
        // `i - rmax` becomes a large positive for `i + 1 < bounded_max`, so
        // body iteration is SKIPPED and the negative lookbehind succeeds.
        // For `i + 1 >= bounded_max`, overflow wraps back and body iterates
        // normally. We model this with i32-wrap `pattern_java_max`.
        let re = Regex::new(r"(?su:(?<!(?:(?:[a-f]{0,4}?)??)(?isu:(?u:[\w]*+))))").unwrap();
        let matches = re.find("\r\r\rΑα");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[1].start, 1);
        assert_eq!(matches[2].start, 2);
    }

    #[test]
    fn test_lookbehind_alternation_with_overflowed_alt_does_not_skip() {
        // Regression case from the earlier i32-overflow attempt: when the
        // lookbehind body has an alternation `(empty | unbounded_body)`,
        // Java's `Branch.study` takes signed `Math.max` across atoms — the
        // null/empty atom (=0) dominates the unbounded alt's negative wrap.
        // So `rmax` ends up at 0 (not negative), and `NotBehind` iterates
        // body normally at `j=i`. The empty alt then matches zero-width,
        // making the negative lookbehind FAIL.
        //
        // Earlier attempts that always-skipped iteration when body had any
        // unbounded part broke this case. The fix is to honor Java's
        // `Math.max` across alternation atoms in `pattern_java_max`.
        let re = Regex::new(r"(?<!|\n\t(?:.{4,})(?:))").unwrap();
        assert_eq!(
            re.find("").len(),
            0,
            "alt 1 (empty) matches → neg lookbehind fails at every position"
        );
    }

    #[test]
    fn test_lookbehind_ques_lazy_body_zero_width_match() {
        // Regression from the same investigation: `(?<!(?:X)??)` with X
        // containing an unbounded `+` part. Java compiles `(?:X)??` as
        // `Branch[null, head]`, so even if X's chain overflows the i32
        // maxLength to negative, Branch's Math.max(null=0, X_max=negative)
        // = 0. The lookbehind body's overall maxLength = 0, NotBehind
        // iterates `j=i` normally, body's null path matches zero-width,
        // and the negative lookbehind FAILS.
        //
        // The fix: in `node_java_max`, when Quantified has `max == 1`
        // (Ques), use `inner_max.max(0)` to mirror Java's Branch wrap.
        let re = Regex::new(r"(?!(?<!(?:\n\z\R(?:\Q~\r\E+))??)+?)").unwrap();
        let matches = re.find("\r");
        // The outer is a positive lookahead of negated... complex. Just
        // verify count matches Java (2 for input "\r").
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_groupcurly_backoff_overrides_capture_with_iter_slice() {
        // Reduced from a fuzz-found case (seed 20). Pattern:
        // `(?:([^\w])+){2}` on "\t\t\r".
        //
        // Java compiles the outer `(?:X){2}` (non-det inner) to `Prolog(Loop)`
        // and the inner `([^\w])+` to a capturing `GroupCurly`. When the inner
        // GroupCurly's greedy `match0` maximally consumes 3 chars then needs
        // to back off for the outer Loop's 2nd iter to fit, the backoff loop
        // explicitly OVERRIDES groups[idx,+1] = (i-k, i) — the slice of the
        // last-kept atom — AFTER `next.match` succeeds. Even though the
        // recursive Loop iter 2 internally re-sets groups[idx] = (2,3), the
        // outer GroupCurly's override re-stamps to (1,2) on return. Final
        // groups[1] = "\t".
        //
        // Our match_greedy mirrors this: after `match_nodes(rest)` succeeds at
        // a count > 0, we override captures[idx] = (iter_start, pos) — the
        // slice of the last consumed iter. iter_start is passed down through
        // each recursive try_match_atom_greedy → match_greedy hop.
        let re = Regex::new(r"(?:([^\w])+){2}").unwrap();
        let ms = re.find("\t\t\r");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].group(0), Some("\t\t\r"));
        assert_eq!(
            ms[0].group(1),
            Some("\t"),
            "Java's outer GroupCurly backoff overrides group 1 to its own \
             iter's slice (1,2) after the recursive Loop iter 2 had set (2,3)"
        );
    }

    #[test]
    fn test_reluctant_generic_zero_width_body_aborts_does_not_retry_rest() {
        // Reduced from a fuzz-found case (seed 32). Pattern:
        // `(?:(?<=())*?)(?msu:\1)` on "".
        //
        // The reluctant `*?` is on a Lookbehind (atomic from the perspective
        // of node_is_deterministic — lookarounds are deterministic). atom is
        // Lookbehind, NOT Group, so try_match_atom_reluctant's generic `_` arm
        // handles it. Java's Curly LAZY `match1` aborts on zero-width body:
        // `if (i == matcher.last) return false;`. Our generic arm previously
        // tried `rest` after a zero-width body match unconditionally, picking
        // up the lookbehind body's leaked group-1 capture and making `\1`
        // succeed where Java rejects.
        //
        // The fix mirrors the Group arm's existing `count + 1 == min` guard
        // (the cmin-equivalent body match) — apply the same to the generic and
        // FlagGroup arms.
        let re = Regex::new(r"(?:(?<=())*?)(?msu:\1)").unwrap();
        assert!(
            !re.matches(""),
            "reluctant Lookbehind body (zero-width) must abort like Java's \
             match1; the lookbehind's leaked group 1 capture should not enable \
             \\1 to succeed"
        );
    }

    #[test]
    fn test_reluctant_zero_width_body_aborts_does_not_retry_rest() {
        // Reduced from a fuzz-found case. Pattern: `(?:(?iu)(\B))*?\1\R` on
        // "\r". Java's Curly LAZY `match1` tries `next.match` (= `\1\R`)
        // FIRST. With group 1 unset, `\1` fails. Then match1 attempts atom
        // expansion. atom matches zero-width (\B at start of "\r"). The
        // zero-width abort fires: `if (i == matcher.last) return false`.
        // Curly returns false. Overall pattern doesn't match.
        //
        // Previously our reluctant Group arm's zero-width branch was retrying
        // `rest` after Path 1's body match, picking up the leaked group-1
        // capture so `\1` would now succeed. That made us match where Java
        // doesn't. The fix is to NOT retry rest after a zero-width reluctant
        // body match — Java's `next.match` was already tried at the top of
        // `match_reluctant`.
        let re = Regex::new(r"(?:(?iu)(\B))*?\1\R").unwrap();
        assert_eq!(
            re.find("\r").len(),
            0,
            "Java's reluctant Curly aborts on zero-width body, not re-trying \
             rest with the body's leaked capture"
        );
    }

    #[test]
    fn test_possessive_cmin_loop_does_not_abort_on_zero_width() {
        // Reduced from a fuzz-found case. Pattern's body has alternatives
        // where alt 3 is a neg-lookahead whose inner captures group 2 (and
        // leaks the capture before failing), and alt 2 `(?:.)\2` depends on
        // group 2 being set. In iter 1, alt 2 fails (g2 unset); alt 3 fails
        // but leaks; alt 4 (empty) matches zero-width. In iter 2 (re-run of
        // atom.match by the possessive), alt 2 now succeeds because g2 was
        // leaked. Java's Curly possessive cmin loop iterates `min` times
        // unconditionally without a zero-width abort, then `match2` keeps
        // iterating while atom advances. Our previous match_possessive broke
        // after a single zero-width iter, missing iter 2 where state had
        // changed via the leaked capture.
        let re = Regex::new(r"(?<y2>(?:\z|(?:.)\2|(?!(\G{3,}?))||\Q\E)++)").unwrap();
        assert!(
            re.matches("\t"),
            "possessive ++ should iterate again after a zero-width iter because \
             leaked captures from inner alt 3 enable alt 2 to advance in iter 2"
        );
    }

    #[test]
    fn test_optional_inner_anchor_capture_resets_on_failure() {
        // Reduced from a fuzz-found case. Inner of the neg lookahead is an
        // optional capture group containing an `^` anchor (multiline-scoped).
        // At pos 0 the `^` matches zero-width → g1="" via GroupTail. Then `\R`
        // at pos 0 fails. The `?` greedy retries without the optional. `\R`
        // still fails. Java reports g1=null because GroupTail restores its
        // end-marker on the inner failure even though we never advanced; we
        // previously leaked g1="" because our zero-width body path didn't
        // trigger GroupEnd's end-restoration.
        let re = Regex::new(r"(?!(?:(?msu:(^))?)\Q\E(\R))").unwrap();
        let matches = re.find("3\r");
        assert!(!matches.is_empty());
        // First match at pos 0.
        assert_eq!(matches[0].start, 0);
        assert_eq!(matches[0].end, 0);
        assert_eq!(
            matches[0].groups.first().cloned().flatten(),
            None,
            "g1 must be null when zero-width inner capture's continuation failed"
        );
    }

    #[test]
    fn test_lookbehind_unbounded_multichar_body_rejected() {
        // Java: `(?<=(?:ab)+)` and similar reject at compile time.
        assert!(Regex::new(r"(?<=(?:ab)+)").is_err());
        assert!(Regex::new(r"(?<=(?:ab)*)").is_err());
        assert!(Regex::new(r"(?<=(?:ab){3,})").is_err());
        assert!(Regex::new(r"(?<=(ab)+)").is_err());
        assert!(Regex::new(r"(?<=\R+)").is_err());
        assert!(Regex::new(r"(?<=\1)").is_err());
        // But single-char unbounded quantifiers ARE accepted (matches Java).
        assert!(Regex::new(r"(?<=a+)").is_ok());
        assert!(Regex::new(r"(?<=[ab]+)").is_ok());
        assert!(Regex::new(r"(?<=\d*)").is_ok());
        assert!(Regex::new(r"(?<=(a)+)").is_ok());
        assert!(Regex::new(r"(?<=(?:ab){3,5})").is_ok());
    }

    #[test]
    fn test_inline_flags_propagate_across_alternation_branches() {
        // Java treats inline `(?s)` as a compile-time directive: any branch
        // parsed after it sees the flag, regardless of whether the branch
        // containing it actually matches at run time. So `(?s)|.` matches "\n"
        // (branch 1 sets dotall, branch 2's `.` then matches `\n`).
        let r = Regex::new(r"(?s)|.").unwrap();
        assert!(r.matches("\n"));
        let r = Regex::new(r"(?s)xx|.").unwrap();
        assert!(r.matches("\n"));
        // Inside a scoped FlagGroup, the propagation still applies within scope:
        let r = Regex::new(r"(?iu:(?s)|(?:.).\R)").unwrap();
        assert!(r.matches("\nΑ\r"));
        // Scoped-wrap NEGATIVE cases (from QUIRKS.md): wrapping `(?s)` in any
        // group at all stops the propagation — non-capturing, capturing, atomic,
        // and lookaround groups all close the scope.
        assert!(
            !Regex::new(r"(?:(?s))|.").unwrap().matches("\n"),
            "non-cap wrap should scope the (?s)"
        );
        assert!(
            !Regex::new(r"((?s))|.").unwrap().matches("\n"),
            "capturing wrap should scope the (?s)"
        );
        assert!(
            !Regex::new(r"(?>(?s))|.").unwrap().matches("\n"),
            "atomic wrap should scope the (?s)"
        );
        assert!(
            !Regex::new(r"(?=(?s))|.").unwrap().matches("\n"),
            "lookahead wrap should scope the (?s)"
        );
        // And the same with a non-matching branch-1 still leaves branch-2 with
        // default (no DOTALL) flags:
        assert!(!Regex::new(r"(?:(?s)xx)|.").unwrap().matches("\n"));
    }

    #[test]
    fn test_chained_intersection_mirrors_openjdk_quirk() {
        // In `[A && [P]x && C]` where the middle operand contains a nested
        // class followed by a literal, OpenJDK does NOT chain the trailing
        // `&& C` at the outer level — the literal triggers a recursive
        // sub-class scope that absorbs `x && C`, leaving the outer intersection
        // as `A && ([P] ∪ (x && C))`. For `[abc && [\w]a && z]` this means
        // the result is `{a,b,c}` (not empty, as a straightforward 3-way
        // intersection would give). Mirroring OpenJDK is intentional.
        let r = Regex::new(r"[abc&&[\w]a&&z]").unwrap();
        let texts: Vec<_> = r
            .find("abcxz")
            .into_iter()
            .map(|m| m.matched_text)
            .collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
        // Sanity: when no literal sits between the nested class and the next &&,
        // the chain IS preserved (proper 3-way intersection, result is empty).
        let r = Regex::new(r"[abc&&[\w]&&z]").unwrap();
        assert!(r.find("abcz").is_empty());
    }

    #[test]
    fn test_case_insensitive_range_outside_ascii_bounds() {
        // `[1-c]` with /i should match 'g' because uppercase 'G' (0x47)
        // is inside the range 0x31..0x63.
        let r = Regex::with_flags("[1-c]", "i").unwrap();
        assert!(r.matches("g"));
        assert!(r.matches("G"));
        let r = Regex::with_flags("[=-e]", "i").unwrap();
        assert!(r.matches("f"));
        // Sanity: a range that doesn't span any uppercase ASCII should still reject.
        let r = Regex::with_flags("[d-e]", "i").unwrap();
        assert!(!r.matches("g"));
    }

    // -----------------------------------------------------------------
    // Coverage-targeted tests. Each verifies an OpenJDK-spec behavior
    // (per java.util.regex.Pattern / Matcher Javadoc and observed JDK
    // behavior) for a code path not exercised by the broader suite.
    // -----------------------------------------------------------------

    #[test]
    fn test_pattern_syntax_error_no_context() {
        // PatternSyntaxError::new constructs an error without source context.
        // Display omits the "near index N\npattern\n  ^" part when there is
        // no pattern attached. Mirrors how Java omits index/source when
        // constructing PatternSyntaxException with only a message.
        let err = PatternSyntaxError::new("boom".to_string());
        assert_eq!(err.message, "boom");
        assert_eq!(err.pattern, "");
        assert_eq!(err.index, 0);
        assert_eq!(format!("{err}"), "boom");
    }

    #[test]
    fn test_match_info_name_and_group_count() {
        // Mirrors Matcher.group(String) and Matcher.groupCount().
        let re = Regex::new(r"(?<user>\w+)@(?<host>\w+\.\w+)").unwrap();
        let ms = re.find("alice@example.com");
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.name("user"), Some("alice"));
        assert_eq!(m.name("host"), Some("example.com"));
        assert_eq!(m.name("nope"), None);
        // Java's groupCount = number of groups EXCLUDING group 0.
        assert_eq!(m.group_count(), 2);
        // No named groups → name() always None.
        let re2 = Regex::new(r"(\w+)").unwrap();
        let m2 = &re2.find("hi")[0];
        assert_eq!(m2.name("anything"), None);
        assert_eq!(m2.group_count(), 1);
    }

    #[test]
    fn test_java_property_names() {
        // OpenJDK supports these Java-specific \p{...} property names
        // (Pattern Javadoc § "Classes for Unicode scripts, blocks, categories
        // and binary properties" — the javaXxx family mirrors
        // java.lang.Character predicates).
        //
        // \p{javaISOControl}: Character.isISOControl — U+0000..U+001F and
        // U+007F..U+009F.
        let r = Regex::new(r"\p{javaISOControl}").unwrap();
        assert!(r.matches("\u{0000}"));
        assert!(r.matches("\u{001F}"));
        assert!(r.matches("\u{007F}"));
        assert!(!r.matches(" "));
        assert!(!r.matches("a"));
        // \p{javaUnicodeIdentifierStart}: Character.isUnicodeIdentifierStart
        // — letters (alphabetic). Same set in our approximation.
        let r = Regex::new(r"\p{javaUnicodeIdentifierStart}").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches("Ω"));
        assert!(!r.matches("1"));
        // \p{javaUnicodeIdentifierPart}: Character.isUnicodeIdentifierPart
        // — letters, digits, and underscore.
        let r = Regex::new(r"\p{javaUnicodeIdentifierPart}").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches("1"));
        assert!(r.matches("_"));
        assert!(!r.matches(" "));
        // \p{javaIdentifierIgnorable}: Character.isIdentifierIgnorable —
        // controls (non-whitespace) and format chars. Java: returns true for
        // U+0007 (BEL, control non-whitespace) and U+200B (ZWSP, Cf).
        let r = Regex::new(r"\p{javaIdentifierIgnorable}").unwrap();
        assert!(r.matches("\u{0007}"));
        assert!(r.matches("\u{200B}"));
        assert!(!r.matches("a"));
        assert!(!r.matches(" "));
    }

    #[test]
    fn test_java_mirrored_property_covers_math_symbols() {
        // OpenJDK's \p{javaMirrored} via Character.isMirrored. Bidi_Mirrored
        // covers Ps/Pe/Pi/Pf punctuation universally; for math symbols (Sm),
        // it covers a curated subset including U+2208 (∈ ELEMENT OF) but
        // NOT U+2200 (∀ FOR ALL).
        let r = Regex::new(r"\p{javaMirrored}").unwrap();
        // Open/close punct — short-circuits in is_bidi_mirrored.
        assert!(r.matches("("));
        assert!(r.matches(")"));
        // Math symbol IN the mirrored set (∈ = U+2208).
        assert!(r.matches("\u{2208}"));
        // Math symbol NOT in the mirrored set (∀ = U+2200) — exercises
        // is_mirrored_math returning false.
        assert!(!r.matches("\u{2200}"));
        // Non-mirrored, non-punctuation, non-math (a letter).
        assert!(!r.matches("a"));
        // ASCII '<' and '>' are handled as a special-case in is_bidi_mirrored
        // (treated as mirrored per Java).
        assert!(r.matches("<"));
        assert!(r.matches(">"));
    }

    #[test]
    fn test_unicode_block_no_match_for_char_outside_block() {
        // \p{InBasicLatin} matches ASCII only; a non-ASCII char hits the
        // "Some(b) => b != expected" branch returning false.
        let r = Regex::new(r"\p{InBasicLatin}").unwrap();
        assert!(r.matches("a"));
        // Cyrillic 'я' (U+044F) is in Cyrillic block, not BasicLatin.
        assert!(!r.matches("я"));
    }

    #[test]
    fn test_script_short_name_resolution() {
        // OpenJDK accepts both full and short ISO 15924 script names with
        // the "Is" prefix. Latn = Latin (short name path).
        let r = Regex::new(r"\p{IsLatn}").unwrap();
        assert!(r.matches("a"));
        assert!(!r.matches("я")); // Cyrillic
        let r = Regex::new(r"\p{IsLatin}").unwrap();
        assert!(r.matches("a"));
    }

    #[test]
    fn test_match_unicode_property_public_wrapper() {
        // The non-_ext convenience wrapper. Calls match_unicode_property_ext
        // with unicode_class=false. Tests the public API surface.
        use crate::unicode::match_unicode_property;
        assert!(match_unicode_property("digit", '5'));
        assert!(!match_unicode_property("digit", 'a'));
        assert!(match_unicode_property("L", 'a'));
    }

    #[test]
    fn test_inline_unix_lines_flag() {
        // OpenJDK inline `(?d)` sets UNIX_LINES (treats only `\n` as a line
        // terminator). Documented in java.util.regex.Pattern Javadoc § "Match
        // flags". With UNIX_LINES, `.` matches everything except `\n`, and `^`
        // / `$` in multiline mode anchor only at `\n` — not at `\r` or other
        // line terminators.
        let r = Regex::new(r"(?d).").unwrap();
        // `\r` is NOT a line terminator under UNIX_LINES, so `.` matches it.
        assert!(r.matches("\r"));
        // `\n` IS the terminator under UNIX_LINES; `.` rejects.
        assert!(!r.matches("\n"));
        // Sanity: without `(?d)`, default treats both \r and \n as terminators.
        let r = Regex::new(r".").unwrap();
        assert!(!r.matches("\r"));
        assert!(!r.matches("\n"));
    }

    #[test]
    fn test_char_class_with_predefined_h_v() {
        // [\H] and [\V] inside a character class: Java accepts these as
        // "non-horizontal whitespace" and "non-vertical whitespace" set
        // contributors. The bare `\H` / `\V` are documented predefined
        // classes; using them inside [...] composes them with other items.
        let r = Regex::new(r"[\H]").unwrap();
        assert!(r.matches("a"));
        // Horizontal whitespace per Java: \t and Unicode space separators
        // (\u{00A0}, \u{1680}, etc.) — \H rejects ' ' since space IS \h.
        assert!(!r.matches(" "));
        assert!(!r.matches("\t"));
        // `\r` / `\n` are vertical whitespace, not horizontal — so \H
        // includes them.
        assert!(r.matches("\r"));
        assert!(r.matches("\n"));

        let r = Regex::new(r"[\V]").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches(" ")); // space is horizontal, not vertical → \V includes
        assert!(r.matches("\t")); // tab is horizontal too
        assert!(!r.matches("\n")); // newline IS vertical → \V rejects
        assert!(!r.matches("\u{2028}")); // line separator is vertical → \V rejects
    }

    #[test]
    fn test_grapheme_cluster_with_zwj() {
        // \X matches a single extended grapheme cluster, including ZWJ
        // sequences. OpenJDK supports \X as the Unicode UAX#29 extended
        // grapheme cluster (with some quirks). A "family" emoji like
        // 👨‍👩‍👦 = U+1F468 U+200D U+1F469 U+200D U+1F466 is one cluster.
        let r = Regex::new(r"\X").unwrap();
        let ms = r.find("👨‍👩‍👦");
        assert_eq!(
            ms.len(),
            1,
            "expected the whole ZWJ sequence as one cluster"
        );
        // Combining marks attach to base: "é" with combining acute
        // (a + combining acute) is one cluster.
        let ms = r.find("a\u{0301}");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].matched_text, "a\u{0301}");
    }

    #[test]
    fn test_grapheme_in_lookbehind_unbounded_rejected() {
        // \X is unbounded (1..many chars). Java rejects unbounded multi-char
        // bodies in lookbehind at compile time. node_max_length(\X) = None,
        // so pattern_max_length(lookbehind body) = None → reject.
        assert!(Regex::new(r"(?<=\X)").is_err());
    }

    #[test]
    fn test_zero_width_outer_with_quantified_literal_body() {
        // Triggers pattern_max_length evaluation on a body containing
        // Quantified(Literal,...), exercising the node_max_length arms for
        // Literal (Some(1)), the max==u32::MAX path (None), and the bounded
        // checked_mul path. Outer `(...)*` with min=0,max>1,body matching
        // zero-width.
        // Body = a* → Quantified Literal, max=u32::MAX.
        let r = Regex::new(r"(a*)*").unwrap();
        assert!(r.matches(""));
        // Body = a{3} → Quantified Literal, bounded max.
        let r = Regex::new(r"(a{3})*").unwrap();
        assert!(r.matches(""));
        // Body = \R{2} → Quantified LinebreakMatcher.
        let r = Regex::new(r"(\R{2})*").unwrap();
        assert!(r.matches(""));
        // Body containing FlagGroup quantified.
        let r = Regex::new(r"((?i:a)?)*").unwrap();
        assert!(r.matches(""));
        // Body containing Backreference quantified (unsized → None).
        let r = Regex::new(r"((\1)?)*").unwrap();
        assert!(r.matches(""));
        // Body containing GraphemeCluster quantified.
        let r = Regex::new(r"(\X?)*").unwrap();
        assert!(r.matches(""));
    }

    #[test]
    fn test_case_insensitive_backref_in_reluctant_quantifier() {
        // Backreference arm of try_match_atom_mode in reluctant mode with
        // case-insensitive comparison. Pattern (X)\1? captures "A" then
        // reluctant-attempts \1 with CI. Mirrors Java Matcher behavior.
        let r = Regex::with_flags(r"(A)\1+?", "i").unwrap();
        // Capture "A" then reluctantly match more 'A' (case-insensitive).
        assert!(r.matches("Aa"));
        assert!(r.matches("AA"));
        // CI mismatch — second char isn't 'a' nor 'A'.
        let r = Regex::with_flags(r"(A)\1+?z", "i").unwrap();
        assert!(r.matches("AaZ")); // backref \1 matches "a" (CI), then "Z" matches "z" (CI)
        assert!(!r.matches("Abz")); // 'b' is not CI-equal to 'A'
    }

    #[test]
    fn test_reluctant_flag_group_zero_width_body() {
        // Reluctant quantifier on a FlagGroup with a non-deterministic body
        // (so chain-based). Exercises the FlagGroup reluctant chain arm in
        // try_match_atom_mode.
        let r = Regex::new(r"(?i:a|b)*?c").unwrap();
        assert!(r.matches("AAc"));
        assert!(r.matches("c"));
        // Reluctant non-det FlagGroup zero-width body — outer reluctant
        // {2,3}? with body that can match zero-width via lookaround.
        let r = Regex::new(r"(?i:(?=a)|x){0,3}?a").unwrap();
        assert!(r.matches("a"));
    }

    #[test]
    fn test_unicode_javamirrored_math_ranges() {
        // Covers the multi-line matches! macro in is_mirrored_math: one
        // representative char per uncovered range line. Each is a math
        // symbol whose Bidi_Mirrored=Yes per Unicode data.
        let r = Regex::new(r"\p{javaMirrored}").unwrap();
        for ch in &[
            '\u{2224}', // ∤
            '\u{2239}', // ∹
            '\u{225F}', // ≟
            '\u{2264}', // ≤
            '\u{228F}', // ⊏
            '\u{22BE}', // ⊾
            '\u{22D0}', // ⋐
            '\u{22F0}', // ⋰
            '\u{2320}', // ⌠
            '\u{27D5}', // ⟕
            '\u{27E2}', // ⟢
        ] {
            assert!(
                r.matches(&ch.to_string()),
                "U+{:04X} should be javaMirrored (Bidi_Mirrored math symbol)",
                *ch as u32
            );
        }
    }

    #[test]
    fn test_replace_multi_digit_group_and_literal_dollar() {
        // OpenJDK's appendReplacement DSL: `$NN` references group NN, with
        // greedy multi-digit consumption capped at the actual group count
        // (so for 1-group pattern, `$11` is `$1` + literal "1"). Literal
        // `$` is `\$`.
        let r = Regex::new(r"(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)").unwrap();
        // $10 → group 10 char (with 11 groups available).
        assert_eq!(r.replace_all("abcdefghijk", "$10-$1"), "j-a");
        // 1-group pattern: `$11` should consume `$1` and leave literal "1",
        // exercising the multi-digit overflow break.
        let r = Regex::new(r"(a)").unwrap();
        assert_eq!(r.replace_all("a", "$11"), "a1");
        // Escaped literal `$` via `\$`.
        let r = Regex::new(r"a").unwrap();
        assert_eq!(r.replace_all("a", r"\$"), "$");
    }

    #[test]
    fn test_replacer_impls_for_string_and_ref_string() {
        // The Replacer trait has impls for `&str`, `String`, and `&String`.
        // Tests directly cover the String / &String impls (the &str impl is
        // exercised by every other test).
        let r = Regex::new(r"x").unwrap();
        let s_owned: String = "Y".to_string();
        let s_ref: &String = &s_owned;
        // Closure impl (FnMut)
        assert_eq!(
            r.replace_all("axb", |_m: &MatchInfo| "Y".to_string()),
            "aYb"
        );
        // String impl
        assert_eq!(r.replace_all("axb", s_owned.clone()), "aYb");
        // &String impl
        assert_eq!(r.replace_all("axb", s_ref), "aYb");
    }

    #[test]
    fn test_negative_lookbehind_unbounded_body_rejected() {
        // Java rejects unbounded multi-char bodies for negative lookbehind
        // (same constraint as positive): `(?<!(?:ab)+)` etc. — error.
        assert!(Regex::new(r"(?<!(?:ab)+)").is_err());
        assert!(Regex::new(r"(?<!\R+)").is_err());
    }

    #[test]
    fn test_char_class_with_comments_mode() {
        // OpenJDK's (?x) COMMENTS flag strips whitespace and `#` comments
        // inside `[...]` as well as outside. Inside the class, whitespace
        // should be ignored.
        let r = Regex::new(r"(?x)[a b c]").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches("b"));
        assert!(!r.matches(" "));
        // Comment inside char class.
        let r = Regex::new("(?x)[a # this is a comment\nb]").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches("b"));
    }

    #[test]
    fn test_unclosed_character_class_error() {
        // Java rejects unclosed character classes. Parser must emit an
        // "Unclosed character class" PatternSyntaxException.
        assert!(Regex::new(r"[abc").is_err());
        assert!(Regex::new(r"[a-").is_err());
    }

    #[test]
    fn test_empty_intersection_error() {
        // OpenJDK rejects `[abc&&]` — the RHS of `&&` cannot be empty.
        assert!(Regex::new(r"[abc&&]").is_err());
    }

    #[test]
    fn test_nested_character_class() {
        // OpenJDK supports nested character classes: `[[abc][xyz]]` is the
        // union of {a,b,c} and {x,y,z}, equivalent to `[abcxyz]`.
        let r = Regex::new(r"[[abc][xyz]]").unwrap();
        assert!(r.matches("a"));
        assert!(r.matches("b"));
        assert!(r.matches("x"));
        assert!(r.matches("y"));
        assert!(!r.matches("m"));
        // Nested with negation: `[[^abc]]` is "anything not in abc".
        let r = Regex::new(r"[[^abc]]").unwrap();
        assert!(r.matches("d"));
        assert!(!r.matches("a"));
    }

    #[test]
    fn test_unknown_category_name_returns_false() {
        // match_ugc_category falls through `_ => false` for unrecognized
        // names. Java's \p{...} with an unknown name is rejected at parse
        // time, so we can only hit this branch via the engine's internal
        // category lookup. Exercise via a property name our parser accepts
        // but the category matcher doesn't recognize.
        use crate::unicode::match_unicode_property;
        // Pass a name that goes through to match_ugc_category but isn't in
        // the giant match arm — falls through to false.
        assert!(!match_unicode_property(
            "definitely_not_a_real_category",
            'a'
        ));
    }

    #[test]
    fn test_find_iter_iterator_api() {
        // Exercises Matches::next + find_iter. Mirrors using Java's
        // `while (m.find()) { ... }` loop. Equivalent to find() but yields
        // matches lazily.
        let re = Regex::new(r"\w+").unwrap();
        let collected: Vec<_> = re
            .find_iter("hello   world\tfoo")
            .map(|m| m.matched_text)
            .collect();
        assert_eq!(collected, vec!["hello", "world", "foo"]);
        // Zero-width matches at each position — mirrors Java's behavior
        // where m.find() advances by 1 after a zero-width match.
        let re = Regex::new(r"\b").unwrap();
        let count = re.find_iter("ab cd").count();
        assert!(count >= 4, "expected ≥4 word boundaries, got {count}");
        // Empty input — iterator terminates immediately for a non-zero-width
        // pattern, and yields exactly one zero-width match for one.
        let re = Regex::new(r"\w").unwrap();
        assert_eq!(re.find_iter("").count(), 0);
        let re = Regex::new(r"").unwrap();
        assert_eq!(re.find_iter("").count(), 1);
    }
}
