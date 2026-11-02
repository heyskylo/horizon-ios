// Copyright (c) 2025 Mathieu Dutour Sikiric
// SPDX-License-Identifier: MIT OR Apache-2.0
// Horizon compatibility changes are documented in ../HORIZON.md.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::types::*;
use crate::unicode::is_valid_unicode_property;

/// Compute the maximum match length of a Pattern, or None if the engine cannot
/// prove a finite upper bound. Mirrors `TreeInfo.maxValid` / `maxLength`
/// propagation in OpenJDK `Pattern.java`. Used by lookbehind compile-time
/// validation: Java rejects lookbehind bodies for which this is None (the
/// "Look-behind group does not have an obvious maximum length" error).
fn pattern_max_length(p: &Pattern) -> Option<usize> {
    let mut max = 0;
    for branch in &p.branches {
        let mut total: usize = 0;
        for node in branch {
            total = total.checked_add(node_max_length(node)?)?;
        }
        if total > max {
            max = total;
        }
    }
    Some(max)
}

fn node_max_length(n: &Node) -> Option<usize> {
    match n {
        Node::Literal(_) | Node::Dot | Node::CharClass(_) => Some(1),
        // \R matches \r\n (2 chars) or a single line-break char (1 char).
        Node::LinebreakMatcher => Some(2),
        Node::Anchor(_)
        | Node::SetFlags(_)
        | Node::RestoreFlags(_)
        | Node::Lookahead { .. }
        | Node::Lookbehind { .. } => Some(0),
        Node::Group { inner, .. } | Node::FlagGroup { inner, .. } | Node::AtomicGroup { inner } => {
            pattern_max_length(inner)
        }
        Node::Quantified { inner, max, .. } => {
            // Mirrors OpenJDK's Curly.study/GroupCurly.study overflow check.
            // For an unbounded count, the body's max length must be at most 1
            // (otherwise atom_max * MAX_REPS overflows i32 in OpenJDK, which
            // is the failure they detect — see `temp < maxL` test). The Rust
            // equivalent is "cannot fit in usize when multiplied by MAX_REPS";
            // we use 1 directly as the rule for clarity and compatibility.
            let inner_max = node_max_length(inner)?;
            if *max == u32::MAX {
                if inner_max <= 1 {
                    Some(inner_max)
                } else {
                    None
                }
            } else {
                inner_max.checked_mul(*max as usize)
            }
        }
        // Backrefs cannot be sized at compile time (length depends on what
        // the referenced group matches at runtime). OpenJDK rejects too.
        Node::Backreference(_) | Node::NamedBackreference(_) => None,
        Node::GraphemeCluster => None,
        // The parser's `pattern_max_length` is called via
        // `is_lookbehind_bounded` on a freshly-parsed pattern body. That
        // body comes straight from the parser and never contains engine-
        // internal nodes (GroupEnd, GreedyCont, ReluctantCont,
        // PositionCheck), which are produced only during matching.
        n => unreachable!(
            "parser::node_max_length called with engine-internal node {n:?} \
             — only invoked on freshly-parsed pattern bodies which never \
             contain such nodes"
        ),
    }
}

fn is_lookbehind_bounded(p: &Pattern) -> bool {
    pattern_max_length(p).is_some()
}

pub struct Parser {
    chars: Vec<char>,
    pos: usize,
    pub flags: Flags,
    pub group_count: usize,
    pub named_groups: BTreeMap<String, usize>,
    all_named_backrefs: Vec<String>,
    name_resolver: Option<fn(&str) -> Option<u32>>,
}

impl Parser {
    pub fn new(
        pattern: &str,
        flags: Flags,
        name_resolver: Option<fn(&str) -> Option<u32>>,
    ) -> Self {
        Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            flags,
            group_count: 0,
            named_groups: BTreeMap::new(),
            all_named_backrefs: Vec::new(),
            name_resolver,
        }
    }

    pub fn parse(
        mut self,
    ) -> Result<(Pattern, usize, BTreeMap<String, usize>), PatternSyntaxError> {
        let pattern = self.parse_pattern()?;
        if self.pos < self.chars.len() {
            return Err(self.error(format!(
                "Unexpected character '{}' at position {}",
                self.chars[self.pos], self.pos
            )));
        }
        for name in &self.all_named_backrefs {
            if !self.named_groups.contains_key(name) {
                return Err(self.error(format!("Unknown named group: {}", name)));
            }
        }
        let gc = self.group_count;
        let ng = self.named_groups;
        Ok((pattern, gc, ng))
    }

    /// Construct a PatternSyntaxError carrying the current pattern + cursor
    /// position. The Display impl formats this Java-style:
    ///     `<message> near index <N>\n<pattern>\n      ^`
    fn error(&self, message: String) -> PatternSyntaxError {
        PatternSyntaxError::with_context(message, self.chars.iter().collect(), self.pos)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn expect(&mut self, expected: char) -> Result<(), PatternSyntaxError> {
        match self.advance() {
            Some(c) if c == expected => Ok(()),
            _ => Err(self.error(format!("Expected '{}'", expected))),
        }
    }

    fn parse_pattern(&mut self) -> Result<Pattern, PatternSyntaxError> {
        // Java's inline `(?s)` is compile-time: any branch parsed AFTER an
        // inline flag setter sees the new flags at parse time (because the
        // parser's `self.flags` mutates and Dot/etc. in subsequent branches are
        // compiled against that). At runtime, however, the engine evaluates
        // flags dynamically, so we need each branch to RESET runtime flags to
        // the parser-time state at the start of that branch. We do this by
        // prepending a `SetFlags(branch_start_flags)` node to each branch.
        // (`SetFlags` no longer rolls back on failure, so this also makes
        // earlier-branch inline-flag changes visible to later branches —
        // matching Java's behavior where `(?s)|.` matches "\n".)
        let mut branches = Vec::new();
        loop {
            let branch_start_flags = self.flags;
            let mut branch = self.parse_branch()?;
            branch.insert(0, Node::SetFlags(branch_start_flags));
            branches.push(branch);
            if self.peek() != Some('|') {
                break;
            }
            self.advance();
        }
        Ok(Pattern { branches })
    }

    fn parse_branch(&mut self) -> Result<Vec<Node>, PatternSyntaxError> {
        let mut nodes = Vec::new();
        loop {
            if self.flags.comments {
                self.skip_comments_whitespace();
            }
            match self.peek() {
                None => break,
                Some('|') | Some(')') => break,
                _ => {}
            }
            // Handle \Q...\E specially: emit all-but-last as literals,
            // let only the last go through quantifier parsing
            if self.pos + 1 < self.chars.len()
                && self.chars[self.pos] == '\\'
                && self.chars[self.pos + 1] == 'Q'
            {
                self.pos += 2;
                let mut quoted_chars = Vec::new();
                loop {
                    if self.pos >= self.chars.len() {
                        break;
                    }
                    if self.pos + 1 < self.chars.len()
                        && self.chars[self.pos] == '\\'
                        && self.chars[self.pos + 1] == 'E'
                    {
                        self.pos += 2;
                        break;
                    }
                    quoted_chars.push(self.chars[self.pos]);
                    self.pos += 1;
                }
                if quoted_chars.is_empty() {
                    continue;
                }
                for &ch in &quoted_chars[..quoted_chars.len() - 1] {
                    nodes.push(Node::Literal(ch));
                }
                let last = Node::Literal(*quoted_chars.last().unwrap());
                let node = self.maybe_parse_quantifier(last)?;
                nodes.push(node);
                continue;
            }
            let node = self.parse_atom()?;
            let node = self.maybe_parse_quantifier(node)?;
            nodes.push(node);
        }
        Ok(nodes)
    }

    fn skip_comments_whitespace(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c == ' ' || c == '\t' || c == '\n' || c == '\r' => {
                    self.advance();
                }
                Some('#') => {
                    self.advance();
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            self.advance();
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn parse_atom(&mut self) -> Result<Node, PatternSyntaxError> {
        let c = self
            .peek()
            .ok_or_else(|| self.error("Unexpected end of pattern".to_string()))?;

        match c {
            '\\' => self.parse_escape(),
            '.' => {
                self.advance();
                Ok(Node::Dot)
            }
            '^' => {
                self.advance();
                Ok(Node::Anchor(AnchorKind::StartOfLine))
            }
            '$' => {
                self.advance();
                Ok(Node::Anchor(AnchorKind::EndOfLine))
            }
            '[' => self.parse_char_class_node(),
            '(' => self.parse_group(),
            '*' | '+' | '?' => Err(self.error(format!("Dangling meta character '{}'", c))),
            '{' => {
                let saved = self.pos;
                self.advance();
                match self.parse_quantifier_braces() {
                    Ok((min, max)) => {
                        let kind = match self.peek() {
                            Some('?') => {
                                self.advance();
                                QuantKind::Reluctant
                            }
                            Some('+') => {
                                self.advance();
                                QuantKind::Possessive
                            }
                            _ => QuantKind::Greedy,
                        };
                        let empty = Node::Group {
                            index: None,
                            name: None,
                            inner: Pattern {
                                branches: vec![vec![]],
                            },
                        };
                        Ok(Node::Quantified {
                            inner: Box::new(empty),
                            min,
                            max,
                            kind,
                        })
                    }
                    Err(_) => {
                        self.pos = saved;
                        Err(self.error(format!("Illegal repetition near index {}", self.pos)))
                    }
                }
            }
            _ => {
                self.advance();
                Ok(Node::Literal(c))
            }
        }
    }

    fn parse_escape(&mut self) -> Result<Node, PatternSyntaxError> {
        self.advance(); // consume '\'
        let c = self
            .advance()
            .ok_or_else(|| self.error("Unexpected end of pattern after \\".to_string()))?;

        match c {
            // Predefined character classes
            'd' => Ok(self.predefined_node(PredefinedClass::Digit)),
            'D' => Ok(self.predefined_node(PredefinedClass::NonDigit)),
            'w' => Ok(self.predefined_node(PredefinedClass::Word)),
            'W' => Ok(self.predefined_node(PredefinedClass::NonWord)),
            's' => Ok(self.predefined_node(PredefinedClass::Whitespace)),
            'S' => Ok(self.predefined_node(PredefinedClass::NonWhitespace)),
            'h' => Ok(self.predefined_node(PredefinedClass::HorizWhitespace)),
            'H' => Ok(self.predefined_node(PredefinedClass::NonHorizWhitespace)),
            'v' => Ok(self.predefined_node(PredefinedClass::VertWhitespace)),
            'V' => Ok(self.predefined_node(PredefinedClass::NonVertWhitespace)),

            // Anchors
            'A' => Ok(Node::Anchor(AnchorKind::StartOfInput)),
            'z' => Ok(Node::Anchor(AnchorKind::EndOfInput)),
            'Z' => Ok(Node::Anchor(AnchorKind::EndOfInputBeforeFinalNewline)),
            'b' => {
                if self.peek() == Some('{') {
                    self.advance();
                    self.expect('g')?;
                    self.expect('}')?;
                    return Ok(Node::Anchor(AnchorKind::GraphemeBoundary));
                }
                Ok(Node::Anchor(AnchorKind::WordBoundary))
            }
            'B' => Ok(Node::Anchor(AnchorKind::NonWordBoundary)),
            'G' => Ok(Node::Anchor(AnchorKind::PreviousMatchEnd)),

            // Special escape sequences
            't' => Ok(Node::Literal('\t')),
            'n' => Ok(Node::Literal('\n')),
            'r' => Ok(Node::Literal('\r')),
            'f' => Ok(Node::Literal('\x0C')),
            'a' => Ok(Node::Literal('\x07')),
            'e' => Ok(Node::Literal('\x1B')),

            'R' => Ok(Node::LinebreakMatcher),
            'X' => Ok(Node::GraphemeCluster),
            // Note: top-level `\Q...\E` is pre-handled in parse_pattern_alt;
            // char-class `\Q...\E` is handled in parse_char_class_item. So
            // `\Q` never reaches parse_escape — no arm needed here.
            'p' => self.parse_unicode_property_node(false),
            'P' => self.parse_unicode_property_node(true),
            'N' => self.parse_named_character().map(codepoint_node),
            'x' => self.parse_hex_char().map(codepoint_node),
            'u' => self.parse_unicode_char().map(codepoint_node),
            '0' => self.parse_octal_char().map(Node::Literal),
            'c' => self.parse_control_char().map(Node::Literal),

            // Backreference (numbered)
            '1'..='9' => {
                let mut num = (c as u32 - '0' as u32) as usize;
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() {
                        let new_num = num * 10 + (d as u32 - '0' as u32) as usize;
                        if new_num <= self.group_count {
                            num = new_num;
                            self.advance();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Ok(Node::Backreference(num))
            }

            // Named backreference
            'k' => {
                self.expect('<')?;
                let name = self.parse_group_name()?;
                self.expect('>')?;
                self.all_named_backrefs.push(name.clone());
                Ok(Node::NamedBackreference(name))
            }

            // Note: there's no "bare" 3-digit octal (`\377`) arm here.
            // Java's spec requires the leading `0` (`\0nnn`). The `'1'..='9'`
            // arm above always catches the leading digit as a backreference,
            // matching OpenJDK behavior.

            // Escaped metacharacters
            '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$'
            | '-' | '!' | '=' | '<' | '>' | '/' | '#' | ' ' | '&' | '~' | '@' | '`' | '\''
            | '"' | ',' | ';' | ':' => Ok(Node::Literal(c)),

            'E' => Err(self.error(format!(
                "Illegal/unsupported escape sequence near index {}",
                self.pos - 1
            ))),

            c if c.is_ascii_alphabetic() => {
                Err(self.error("Illegal/unsupported escape sequence".to_string()))
            }
            _ => Ok(Node::Literal(c)),
        }
    }

    // Shared escape-character parsers used by both parse_escape and parse_char_class_item

    fn parse_named_character(&mut self) -> Result<u32, PatternSyntaxError> {
        self.expect('{')?;
        let mut name = String::new();
        loop {
            match self.advance() {
                Some('}') => break,
                Some(c) => name.push(c),
                None => return Err(self.error("Unclosed character name".to_string())),
            }
        }
        self.name_resolver
            .and_then(|resolve| resolve(&name))
            .filter(|&point| point <= 0x10ffff)
            .ok_or_else(|| self.error(format!("Unknown character name: {name}")))
    }

    fn parse_hex_char(&mut self) -> Result<u32, PatternSyntaxError> {
        if self.peek() == Some('{') {
            self.advance();
            let mut hex = String::new();
            while let Some(c) = self.peek() {
                if c == '}' {
                    self.advance();
                    break;
                }
                hex.push(c);
                self.advance();
            }
            let code = u32::from_str_radix(&hex, 16)
                .map_err(|_| self.error(format!("Invalid hex escape: {}", hex)))?;
            if code > 0x10FFFF {
                return Err(self.error("Hexadecimal codepoint is too big".to_string()));
            }
            Ok(code)
        } else {
            let mut hex = String::new();
            for _ in 0..2 {
                if let Some(c) = self.peek() {
                    if c.is_ascii_hexdigit() {
                        hex.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            if hex.len() != 2 {
                return Err(self.error("Invalid hex escape".to_string()));
            }
            let code = u32::from_str_radix(&hex, 16)
                .map_err(|_| self.error(format!("Invalid hex escape: {}", hex)))?;
            Ok(code)
        }
    }

    fn parse_unicode_char(&mut self) -> Result<u32, PatternSyntaxError> {
        let code = self.parse_4hex()?;
        // Handle UTF-16 surrogate pairs: \uD800\uDC00 etc.
        if (0xD800..=0xDBFF).contains(&code) {
            // High surrogate — look for \uXXXX low surrogate
            let saved_pos = self.pos;
            if self.peek() == Some('\\') {
                self.advance();
                if self.peek() == Some('u') {
                    self.advance();
                    if let Ok(low) = self.parse_4hex() {
                        if (0xDC00..=0xDFFF).contains(&low) {
                            let cp = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            return Ok(cp);
                        }
                    }
                }
            }
            self.pos = saved_pos;
        }
        Ok(code)
    }

    fn parse_4hex(&mut self) -> Result<u32, PatternSyntaxError> {
        let mut hex = String::new();
        for _ in 0..4 {
            if let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    hex.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if hex.len() != 4 {
            return Err(self.error(format!("Invalid unicode escape: {}", hex)));
        }
        u32::from_str_radix(&hex, 16)
            .map_err(|_| self.error(format!("Invalid unicode escape: {}", hex)))
    }

    fn parse_octal_char(&mut self) -> Result<char, PatternSyntaxError> {
        // Java's octal: \0 already consumed, read up to 3 more digits.
        // If first digit is 0-3, read up to 2 more (total value ≤ 0377).
        // If first digit is 4-7, read only 1 more (total value ≤ 077).
        let first = match self.peek() {
            Some(c) if ('0'..='7').contains(&c) => {
                self.advance();
                c
            }
            _ => {
                return Err(self.error(format!(
                    "Illegal octal escape sequence near index {}",
                    self.pos
                )));
            }
        };
        let max_more = if ('0'..='3').contains(&first) { 2 } else { 1 };
        let mut oct = String::new();
        oct.push(first);
        for _ in 0..max_more {
            if let Some(c) = self.peek() {
                if ('0'..='7').contains(&c) {
                    oct.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        let code = u32::from_str_radix(&oct, 8).unwrap_or(0);
        Ok(char::from_u32(code).unwrap_or('\0'))
    }

    fn parse_control_char(&mut self) -> Result<char, PatternSyntaxError> {
        let ctrl = self
            .advance()
            .ok_or_else(|| self.error("Expected control character after \\c".to_string()))?;
        let code = (ctrl as u32) ^ 0x40;
        Ok(char::from_u32(code).unwrap_or('\0'))
    }

    fn predefined_node(&self, pc: PredefinedClass) -> Node {
        Node::CharClass(CharClass {
            negated: false,
            items: vec![CharClassItem::Predefined(pc)],
        })
    }

    fn parse_unicode_property_node(&mut self, negated: bool) -> Result<Node, PatternSyntaxError> {
        let (name, neg) = self.parse_property_name(negated)?;
        Ok(Node::CharClass(CharClass {
            negated: false,
            items: vec![CharClassItem::UnicodeProperty { name, negated: neg }],
        }))
    }

    /// Parse a unicode property name (shared between node and char class contexts).
    /// Returns (name, negated).
    fn parse_property_name(&mut self, negated: bool) -> Result<(String, bool), PatternSyntaxError> {
        if self.peek() == Some('{') {
            self.advance();
            let mut name = String::new();
            while let Some(c) = self.peek() {
                if c == '}' {
                    self.advance();
                    break;
                }
                name.push(c);
                self.advance();
            }
            if !is_valid_unicode_property(&name) {
                return Err(self.error(format!("Unknown Unicode property: {}", name)));
            }
            Ok((name, negated))
        } else {
            let c = self
                .advance()
                .ok_or_else(|| self.error("Expected property name after \\p".to_string()))?;
            Ok((c.to_string(), negated))
        }
    }

    fn parse_group_name(&mut self) -> Result<String, PatternSyntaxError> {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                name.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if name.is_empty() {
            return Err(self.error("Empty group name".to_string()));
        }
        if name.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(self.error(format!(
                "Group name must start with a letter, not '{}'",
                name.chars().next().unwrap()
            )));
        }
        Ok(name)
    }

    fn parse_group(&mut self) -> Result<Node, PatternSyntaxError> {
        self.advance(); // consume '('

        if self.peek() == Some('?') {
            self.advance();
            match self.peek() {
                Some(':') => {
                    self.advance();
                    // Java quirk: inline `(?s)` only leaks across alternation
                    // at the *top* level. Inside any group, flag changes are
                    // scoped to that group. Save/restore here so `(?:(?s))|.`
                    // doesn't propagate the `s` to the second branch.
                    let saved = self.flags;
                    let inner = self.parse_pattern()?;
                    self.flags = saved;
                    self.expect(')')?;
                    Ok(Node::Group {
                        index: None,
                        name: None,
                        inner,
                    })
                }
                Some('<') => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            let saved = self.flags;
                            let inner = self.parse_pattern()?;
                            self.flags = saved;
                            self.expect(')')?;
                            if !is_lookbehind_bounded(&inner) {
                                return Err(self.error(
                                    "Look-behind group does not have an obvious maximum length"
                                        .to_string(),
                                ));
                            }
                            Ok(Node::Lookbehind {
                                positive: true,
                                inner,
                            })
                        }
                        Some('!') => {
                            self.advance();
                            let saved = self.flags;
                            let inner = self.parse_pattern()?;
                            self.flags = saved;
                            self.expect(')')?;
                            if !is_lookbehind_bounded(&inner) {
                                return Err(self.error(
                                    "Look-behind group does not have an obvious maximum length"
                                        .to_string(),
                                ));
                            }
                            Ok(Node::Lookbehind {
                                positive: false,
                                inner,
                            })
                        }
                        _ => {
                            let name = self.parse_group_name()?;
                            self.expect('>')?;
                            if self.named_groups.contains_key(&name) {
                                return Err(self.error(format!("Duplicate group name: {}", name)));
                            }
                            self.group_count += 1;
                            let index = self.group_count;
                            self.named_groups.insert(name.clone(), index);
                            let saved = self.flags;
                            let inner = self.parse_pattern()?;
                            self.flags = saved;
                            self.expect(')')?;
                            Ok(Node::Group {
                                index: Some(index),
                                name: Some(name),
                                inner,
                            })
                        }
                    }
                }
                Some('=') => {
                    self.advance();
                    let saved = self.flags;
                    let inner = self.parse_pattern()?;
                    self.flags = saved;
                    self.expect(')')?;
                    Ok(Node::Lookahead {
                        positive: true,
                        inner,
                    })
                }
                Some('!') => {
                    self.advance();
                    let saved = self.flags;
                    let inner = self.parse_pattern()?;
                    self.flags = saved;
                    self.expect(')')?;
                    Ok(Node::Lookahead {
                        positive: false,
                        inner,
                    })
                }
                Some('>') => {
                    self.advance();
                    let saved = self.flags;
                    let inner = self.parse_pattern()?;
                    self.flags = saved;
                    self.expect(')')?;
                    Ok(Node::AtomicGroup { inner })
                }
                _ => self.parse_inline_flags(),
            }
        } else {
            self.group_count += 1;
            let index = self.group_count;
            let saved = self.flags;
            let inner = self.parse_pattern()?;
            self.flags = saved;
            self.expect(')')?;
            Ok(Node::Group {
                index: Some(index),
                name: None,
                inner,
            })
        }
    }

    fn parse_inline_flags(&mut self) -> Result<Node, PatternSyntaxError> {
        let mut set_flags = Flags::default();
        let mut clear_flags = Flags::default();
        let mut clearing = false;

        loop {
            match self.peek() {
                Some(ch @ ('i' | 'm' | 's' | 'x' | 'U' | 'd' | 'u')) => {
                    self.advance();
                    let target = if clearing {
                        &mut clear_flags
                    } else {
                        &mut set_flags
                    };
                    match ch {
                        'i' => target.case_insensitive = true,
                        'm' => target.multiline = true,
                        's' => target.dotall = true,
                        'x' => target.comments = true,
                        'U' => target.unicode_class = true,
                        'd' => target.unix_lines = true,
                        'u' => target.unicode_case = true,
                        // The outer arm at line 602 binds `ch` from the
                        // exhaustive `'i' | 'm' | 's' | 'x' | 'U' | 'd' | 'u'`
                        // pattern, so this catchall is structurally
                        // unreachable. Kept to satisfy the exhaustiveness
                        // checker on the inner `match ch`.
                        ch => unreachable!(
                            "inline-flag char {ch:?} not in the outer pattern \
                             'i' | 'm' | 's' | 'x' | 'U' | 'd' | 'u'"
                        ),
                    }
                }
                Some('-') => {
                    self.advance();
                    clearing = true;
                }
                Some(':') => {
                    self.advance();
                    let saved = self.flags;
                    self.apply_flags(set_flags, clear_flags);
                    let active_flags = self.flags;
                    let inner = self.parse_pattern()?;
                    self.flags = saved;
                    self.expect(')')?;
                    return Ok(Node::FlagGroup {
                        flags: active_flags,
                        inner,
                    });
                }
                Some(')') => {
                    self.advance();
                    self.apply_flags(set_flags, clear_flags);
                    return Ok(Node::SetFlags(self.flags));
                }
                _ => {
                    return Err(self.error("Invalid inline flag".to_string()));
                }
            }
        }
    }

    fn apply_flags(&mut self, set: Flags, clear: Flags) {
        if set.case_insensitive {
            self.flags.case_insensitive = true;
        }
        if set.multiline {
            self.flags.multiline = true;
        }
        if set.dotall {
            self.flags.dotall = true;
        }
        if set.comments {
            self.flags.comments = true;
        }
        if set.unicode_class {
            self.flags.unicode_class = true;
        }
        if set.unix_lines {
            self.flags.unix_lines = true;
        }
        if set.unicode_case {
            self.flags.unicode_case = true;
        }
        if clear.case_insensitive {
            self.flags.case_insensitive = false;
        }
        if clear.multiline {
            self.flags.multiline = false;
        }
        if clear.dotall {
            self.flags.dotall = false;
        }
        if clear.comments {
            self.flags.comments = false;
        }
        if clear.unicode_class {
            self.flags.unicode_class = false;
        }
        if clear.unix_lines {
            self.flags.unix_lines = false;
        }
        if clear.unicode_case {
            self.flags.unicode_case = false;
        }
    }

    fn maybe_parse_quantifier(&mut self, node: Node) -> Result<Node, PatternSyntaxError> {
        if self.flags.comments {
            self.skip_comments_whitespace();
        }
        let (min, max) = match self.peek() {
            Some('*') => {
                self.advance();
                (0, u32::MAX)
            }
            Some('+') => {
                self.advance();
                (1, u32::MAX)
            }
            Some('?') => {
                self.advance();
                (0, 1)
            }
            Some('{') => {
                self.advance();
                match self.parse_quantifier_braces() {
                    Ok((min, max)) => (min, max),
                    Err(_) => {
                        return Err(
                            self.error(format!("Illegal repetition near index {}", self.pos))
                        );
                    }
                }
            }
            _ => return Ok(node),
        };

        let kind = match self.peek() {
            Some('?') => {
                self.advance();
                QuantKind::Reluctant
            }
            Some('+') => {
                self.advance();
                QuantKind::Possessive
            }
            _ => QuantKind::Greedy,
        };

        Ok(Node::Quantified {
            inner: Box::new(node),
            min,
            max,
            kind,
        })
    }

    fn parse_quantifier_braces(&mut self) -> Result<(u32, u32), PatternSyntaxError> {
        let mut min_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                min_str.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if min_str.is_empty() {
            return Err(self.error("Invalid quantifier".to_string()));
        }
        let min: u32 = min_str
            .parse()
            .map_err(|_| self.error("Invalid quantifier number".to_string()))?;
        if min > i32::MAX as u32 {
            return Err(self.error("Illegal repetition range".to_string()));
        }

        match self.peek() {
            Some('}') => {
                self.advance();
                Ok((min, min))
            }
            Some(',') => {
                self.advance();
                if self.peek() == Some('}') {
                    self.advance();
                    Ok((min, u32::MAX))
                } else {
                    let mut max_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            max_str.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect('}')?;
                    let max: u32 = max_str
                        .parse()
                        .map_err(|_| self.error("Invalid quantifier number".to_string()))?;
                    if max > i32::MAX as u32 || min > max {
                        return Err(
                            self.error(format!("Illegal repetition range near index {}", self.pos))
                        );
                    }
                    Ok((min, max))
                }
            }
            _ => Err(self.error("Invalid quantifier".to_string())),
        }
    }

    // ==================== Character Class Parsing ====================

    fn parse_char_class_node(&mut self) -> Result<Node, PatternSyntaxError> {
        let cc = self.parse_char_class()?;
        Ok(Node::CharClass(cc))
    }

    fn parse_char_class(&mut self) -> Result<CharClass, PatternSyntaxError> {
        self.expect('[')?;
        let negated = if self.peek() == Some('^') {
            self.advance();
            true
        } else {
            false
        };
        let items = self.parse_char_class_body(true)?;
        Ok(CharClass { negated, items })
    }

    /// Parse the body of a `[...]` class, mirroring OpenJDK's `clazz(consume)`.
    ///
    /// The RHS of `&&` is parsed recursively via `parse_char_class_body(false)`,
    /// which is responsible for handling its own `&&` and stopping at the
    /// enclosing `]` without consuming it. This faithfully reproduces an
    /// OpenJDK quirk: in `[A && [P]x && C]`, the trailing `&& C` is not chained
    /// at the outer level — the literal `x` triggers a recursive sub-class scope
    /// that absorbs `x && C`, and the resulting (B-with-nested-intersection)
    /// becomes a single right operand of the outer `&&`.
    fn parse_char_class_body(
        &mut self,
        consume_closing: bool,
    ) -> Result<Vec<CharClassItem>, PatternSyntaxError> {
        let mut items = Vec::new();
        let mut at_start = true;

        loop {
            if self.flags.comments {
                self.skip_comments_whitespace();
            }
            match self.peek() {
                None => return Err(self.error("Unclosed character class".to_string())),
                Some(']') if at_start => {
                    self.advance();
                    items.push(CharClassItem::Single(']' as u32));
                    at_start = false;
                    continue;
                }
                Some(']') => {
                    if consume_closing {
                        self.advance();
                    }
                    return Ok(items);
                }
                Some('[') => {
                    let nested = self.parse_char_class()?;
                    items.push(CharClassItem::Nested(nested));
                    at_start = false;
                    continue;
                }
                Some('&') if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '&' => {
                    self.advance();
                    self.advance();
                    let right_items = self.parse_intersection_rhs()?;
                    items = vec![CharClassItem::Intersection(items, right_items)];
                    at_start = false;
                    // Don't return — continue the outer loop. A subsequent `&&`
                    // at this level (when no literal in the RHS triggered the
                    // recursive sub-scope) will produce a left-associative chain.
                    continue;
                }
                _ => {
                    at_start = false;
                    let item = self.parse_char_class_item()?;
                    if self.peek() == Some('-')
                        && self.pos + 1 < self.chars.len()
                        && self.chars[self.pos + 1] != ']'
                    {
                        if let CharClassItem::Single(start) = item {
                            self.advance();
                            let end_item = self.parse_char_class_item()?;
                            if let CharClassItem::Single(end) = end_item {
                                if start > end {
                                    return Err(
                                        self.error(format!("Invalid range: {}-{}", start, end))
                                    );
                                }
                                items.push(CharClassItem::Range(start, end));
                                continue;
                            } else {
                                return Err(self.error("Illegal character range".to_string()));
                            }
                        }
                    }
                    items.push(item);
                }
            }
        }
    }

    /// Parse the right-hand side of `&&`. Nested `[...]` groups are taken as
    /// single items; encountering a literal triggers a recursive sub-class
    /// scope (`parse_char_class_body(false)`), which absorbs the rest of the
    /// current `]` group — including any further `&&` clauses.
    fn parse_intersection_rhs(&mut self) -> Result<Vec<CharClassItem>, PatternSyntaxError> {
        let mut items = Vec::new();
        loop {
            if self.flags.comments {
                self.skip_comments_whitespace();
            }
            match self.peek() {
                None => return Err(self.error("Unclosed character class".to_string())),
                Some(']') => break,
                Some('&') if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '&' => {
                    break
                }
                Some('[') => {
                    let nested = self.parse_char_class()?;
                    items.push(CharClassItem::Nested(nested));
                }
                _ => {
                    // Delegate the rest of the RHS to a recursive sub-class scope.
                    // The recursive call returns at `]` without consuming it, and
                    // handles its own `&&` (which therefore does NOT chain at this
                    // outer scope — this is the OpenJDK parser quirk).
                    let inner = self.parse_char_class_body(false)?;
                    items.extend(inner);
                }
            }
        }
        if items.is_empty() {
            return Err(self.error("Empty intersection operand".to_string()));
        }
        Ok(items)
    }

    fn parse_char_class_item(&mut self) -> Result<CharClassItem, PatternSyntaxError> {
        match self.peek() {
            Some('\\') => {
                self.advance();
                let c = self
                    .advance()
                    .ok_or_else(|| self.error("Unexpected end in character class".to_string()))?;
                match c {
                    'd' => Ok(CharClassItem::Predefined(PredefinedClass::Digit)),
                    'D' => Ok(CharClassItem::Predefined(PredefinedClass::NonDigit)),
                    'w' => Ok(CharClassItem::Predefined(PredefinedClass::Word)),
                    'W' => Ok(CharClassItem::Predefined(PredefinedClass::NonWord)),
                    's' => Ok(CharClassItem::Predefined(PredefinedClass::Whitespace)),
                    'S' => Ok(CharClassItem::Predefined(PredefinedClass::NonWhitespace)),
                    'h' => Ok(CharClassItem::Predefined(PredefinedClass::HorizWhitespace)),
                    'H' => Ok(CharClassItem::Predefined(
                        PredefinedClass::NonHorizWhitespace,
                    )),
                    'v' => Ok(CharClassItem::Predefined(PredefinedClass::VertWhitespace)),
                    'V' => Ok(CharClassItem::Predefined(
                        PredefinedClass::NonVertWhitespace,
                    )),
                    'p' => {
                        let (name, negated) = self.parse_property_name(false)?;
                        Ok(CharClassItem::UnicodeProperty { name, negated })
                    }
                    'P' => {
                        let (name, negated) = self.parse_property_name(true)?;
                        Ok(CharClassItem::UnicodeProperty { name, negated })
                    }
                    't' => Ok(CharClassItem::Single('\t' as u32)),
                    'n' => Ok(CharClassItem::Single('\n' as u32)),
                    'r' => Ok(CharClassItem::Single('\r' as u32)),
                    'f' => Ok(CharClassItem::Single('\x0C' as u32)),
                    'a' => Ok(CharClassItem::Single('\x07' as u32)),
                    'e' => Ok(CharClassItem::Single('\x1B' as u32)),
                    'N' => self.parse_named_character().map(CharClassItem::Single),
                    'x' => self.parse_hex_char().map(CharClassItem::Single),
                    'u' => self.parse_unicode_char().map(CharClassItem::Single),
                    '0' => self
                        .parse_octal_char()
                        .map(|c| CharClassItem::Single(c as u32)),
                    'c' => self
                        .parse_control_char()
                        .map(|c| CharClassItem::Single(c as u32)),
                    'Q' => {
                        let mut items = Vec::new();
                        loop {
                            if self.pos >= self.chars.len() {
                                break;
                            }
                            if self.pos + 1 < self.chars.len()
                                && self.chars[self.pos] == '\\'
                                && self.chars[self.pos + 1] == 'E'
                            {
                                self.pos += 2;
                                break;
                            }
                            items.push(CharClassItem::Single(self.chars[self.pos] as u32));
                            self.pos += 1;
                        }
                        if items.len() == 1 {
                            return Ok(items.into_iter().next().unwrap());
                        }
                        Ok(CharClassItem::Nested(CharClass {
                            negated: false,
                            items,
                        }))
                    }
                    '1'..='9' => Err(self.error(format!(
                        "Illegal backreference in character class near index {}",
                        self.pos - 1
                    ))),
                    c if c.is_ascii_alphabetic() => {
                        Err(self.error("Illegal/unsupported escape sequence".to_string()))
                    }
                    _ => Ok(CharClassItem::Single(c as u32)),
                }
            }
            Some('[') => {
                let nested = self.parse_char_class()?;
                Ok(CharClassItem::Nested(nested))
            }
            Some(c) => {
                self.advance();
                Ok(CharClassItem::Single(c as u32))
            }
            None => Err(self.error("Unexpected end in character class".to_string())),
        }
    }
}

// Rust strings cannot contain unpaired UTF16 surrogates. Keep their numeric
// class predicate (which matches no scalar), rather than replacing them with NUL.
fn codepoint_node(cp: u32) -> Node {
    match char::from_u32(cp) {
        Some(c) => Node::Literal(c),
        None => Node::CharClass(CharClass {
            negated: false,
            items: vec![CharClassItem::Single(cp)],
        }),
    }
}
