//! Drop stage-local functions that neither main nor retained globals can reach.
//! Input has already been preprocessed: comments and conditional branches are gone.
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use super::{anonymous_structs, atomic_vectors, interface_blocks, scan::is_ident};
use crate::{Preprocessed, macros::lex};

struct Token<'a> {
    text: &'a str,
    bytes: Range<usize>,
}

struct Function<'a> {
    name: &'a str,
    tokens: Range<usize>,
    definition: bool,
}

fn closing(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    let (open, close) = match tokens[start].text {
        "(" => ("(", ")"),
        "[" => ("[", "]"),
        "{" => ("{", "}"),
        _ => return None,
    };
    let mut depth = 0;
    for (i, token) in tokens.iter().enumerate().skip(start) {
        if token.text == open {
            depth += 1;
        }
        if token.text == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// A declaration ending in a parameter list, with a return type before its name.
/// An initializer or an unfamiliar header stays untouched rather than being guessed.
fn name<'a>(header: &[Token<'a>]) -> Option<&'a str> {
    if header.last()?.text != ")" {
        return None;
    }
    let mut depth = 0;
    let mut open = None;
    for (i, token) in header.iter().enumerate().rev() {
        match token.text {
            ")" => depth += 1,
            "(" => {
                depth -= 1;
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let at = open?.checked_sub(1)?;
    if at == 0
        || !is_ident(header[at].text)
        || header[..at]
            .iter()
            .any(|t| matches!(t.text, "=" | "," | ";" | "{" | "}" | "(" | ")"))
    {
        return None;
    }
    Some(header[at].text)
}

fn functions<'a>(tokens: &[Token<'a>]) -> Option<Vec<Function<'a>>> {
    let (mut start, mut at) = (0, 0);
    let mut found = Vec::new();
    while at < tokens.len() {
        match tokens[at].text {
            "(" | "[" => at = closing(tokens, at)? + 1,
            "{" => {
                let end = closing(tokens, at)? + 1;
                if let Some(name) = name(&tokens[start..at]) {
                    found.push(Function {
                        name,
                        tokens: start..end,
                        definition: true,
                    });
                    start = end;
                }
                at = end;
            }
            ";" => {
                if let Some(name) = name(&tokens[start..at]) {
                    found.push(Function {
                        name,
                        tokens: start..at + 1,
                        definition: false,
                    });
                }
                at += 1;
                start = at;
            }
            ")" | "]" | "}" => return None,
            _ => at += 1,
        }
    }
    Some(found)
}

fn calls<'a>(tokens: &[Token<'a>]) -> impl Iterator<Item = &'a str> {
    tokens
        .windows(2)
        .filter_map(|pair| (is_ident(pair[0].text) && pair[1].text == "(").then_some(pair[0].text))
}

fn prune(code: &str) -> Option<String> {
    let mut offset = 0;
    let tokens: Vec<_> = lex(code)
        .into_iter()
        .filter_map(|text| {
            let start = offset;
            offset += text.len();
            (!text.trim().is_empty()).then_some(Token {
                text,
                bytes: start..offset,
            })
        })
        .collect();
    let functions = functions(&tokens)?;
    if !functions.iter().any(|f| f.name == "main" && f.definition) {
        return None;
    }
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut inside = vec![false; tokens.len()];
    for (i, f) in functions.iter().enumerate() {
        by_name.entry(f.name).or_default().push(i);
        inside[f.tokens.clone()].fill(true);
    }
    // Globals are retained, including initializers, type/array expressions and pack
    // configuration constants. Calls in them are roots even if main never reads them.
    let mut pending = vec!["main"];
    for i in 0..tokens.len().saturating_sub(1) {
        if !inside[i] && !inside[i + 1] {
            pending.extend(calls(&tokens[i..i + 2]));
        }
    }
    let mut reached = BTreeSet::new();
    let mut keep = vec![false; functions.len()];
    while let Some(name) = pending.pop() {
        if !reached.insert(name) {
            continue;
        }
        if let Some(candidates) = by_name.get(name) {
            // No type guessing: every overload and prototype of a referenced name stays.
            for &i in candidates {
                keep[i] = true;
                pending.extend(calls(&tokens[functions[i].tokens.clone()]));
            }
        }
    }
    if keep.iter().all(|&keep| keep) {
        return None;
    }
    let mut out = code.as_bytes().to_vec();
    for (f, keep) in functions.iter().zip(keep) {
        if !keep {
            let start = tokens[f.tokens.start].bytes.start;
            let end = tokens[f.tokens.end - 1].bytes.end;
            for byte in &mut out[start..end] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
        }
    }
    Some(String::from_utf8(out).expect("whole token spans replaced by spaces"))
}

/// Stage preparation before compatibility analysis. Extracted from translate so its
/// already oversized module does not grow when reachability is added.
pub(super) fn prepare(pp: &Preprocessed) -> Option<Preprocessed> {
    let pruned = prune(&pp.code).map(|code| Preprocessed { code, ..pp.clone() });
    let pp = pruned.as_ref().unwrap_or(pp);
    let named = anonymous_structs::name(&pp.code);
    let code = named.as_deref().unwrap_or(&pp.code);
    let flat = interface_blocks::flatten(code)
        .or(named)
        .map(|code| Preprocessed { code, ..pp.clone() });
    let split = atomic_vectors::rewrite(flat.as_ref().unwrap_or(pp));
    split.or(flat).or(pruned)
}
