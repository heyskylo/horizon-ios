//! `bool` members of a storage buffer. GLSL gives a `bool` in a `layout(std430)` buffer a size and
//! an alignment of 4; naga does not let one into a buffer at all ("not host-shareable"). Such a
//! member becomes a `uint` of the same size and every use of it is rewritten: a read is
//! `(flag != 0u)`, a plain assignment `flag = uint(...)`. The source keeps its lines.
//!
//! What the rewrite cannot make sure of is refused with the member's name and its line, rather
//! than left for naga's unnamed "Global variable [6] '' is invalid": a compound assignment
//! (`flag |= x`), an assignment used as a value, a local or parameter of the same name, a `bool`
//! vector, and an instance array of such a buffer.

use std::collections::BTreeMap;

use super::Error;
use super::scan::{is_ident, significant};
use crate::Preprocessed;
use crate::macros::lex;

/// A `bool` member: where its type is, and its name.
struct Member<'a> {
    type_at: usize,
    name: &'a str,
}

/// One buffer block with `bool` members.
struct Block<'a> {
    /// The `{` and the `}` of the block.
    open: usize,
    close: usize,
    /// The instance name after the `}`, if the block has one.
    instance: Option<&'a str>,
    members: Vec<Member<'a>>,
}

/// `pp` with its buffers' `bool` members rewritten, and what could not be.
pub(super) fn rewrite(pp: &Preprocessed) -> (Option<Preprocessed>, Vec<Error>) {
    if !pp.code.contains("buffer") || !(pp.code.contains("bool") || pp.code.contains("bvec")) {
        return (None, Vec::new());
    }
    let tokens = lex(&pp.code);
    let next = |i: usize| (i + 1..tokens.len()).find(|&j| significant(tokens[j]));
    let prev = |i: usize| (0..i).rev().find(|&j| significant(tokens[j]));
    let line_of = |at: usize| -> Error {
        let line: usize = tokens[..at.min(tokens.len())]
            .iter()
            .map(|t| t.matches('\n').count())
            .sum();
        let o = pp.lines.get(line).copied().unwrap_or_default();
        Error {
            file: pp.files.get(o.file as usize).cloned().unwrap_or_default(),
            line: o.line,
            message: String::new(),
        }
    };
    let mut errors = Vec::new();
    let mut fail = |at: usize, message: String| {
        let mut e = line_of(at);
        e.message = message;
        errors.push(e);
    };
    // The matching close of the bracket at `open`.
    let matching = |open: usize| -> Option<usize> {
        let (up, down) = match tokens[open] {
            "{" => ("{", "}"),
            "[" => ("[", "]"),
            _ => ("(", ")"),
        };
        let mut depth = 0;
        for (j, &t) in tokens.iter().enumerate().skip(open) {
            if t == up {
                depth += 1;
            } else if t == down {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
        }
        None
    };

    // The buffer blocks that have `bool` members.
    let mut blocks: Vec<Block> = Vec::new();
    for (i, &t) in tokens.iter().enumerate() {
        if t != "buffer" {
            continue;
        }
        let Some(open) = (i + 1..tokens.len())
            .take_while(|&j| !matches!(tokens[j], ";" | "(" | "}" | "="))
            .find(|&j| tokens[j] == "{")
        else {
            continue;
        };
        let Some(close) = matching(open) else {
            continue;
        };
        let instance = next(close).filter(|&j| is_ident(tokens[j]));
        let mut members = Vec::new();
        // Statements of the block, split at their `;`.
        let mut start = open + 1;
        while start < close {
            let end = (start..close).find(|&j| tokens[j] == ";").unwrap_or(close);
            let words: Vec<usize> = (start..end).filter(|&j| significant(tokens[j])).collect();
            start = end + 1;
            let mut k = 0;
            // Qualifiers: `layout(...)`, `readonly`...
            while let Some(&w) = words.get(k) {
                if tokens[w] == "layout" {
                    k = words
                        .iter()
                        .position(|&x| Some(x) == matching(words[k + 1]))
                        .map_or(words.len(), |p| p + 1);
                } else if matches!(
                    tokens[w],
                    "readonly" | "writeonly" | "coherent" | "volatile" | "restrict"
                ) || tokens[w].starts_with("highp")
                {
                    k += 1;
                } else {
                    break;
                }
            }
            let Some(&ty) = words.get(k) else { continue };
            let vector = matches!(tokens[ty], "bvec2" | "bvec3" | "bvec4");
            if tokens[ty] != "bool" && !vector {
                continue;
            }
            // Declarators: `name[N]`, separated by commas; `bool[N] name` puts the size first.
            let mut at = k + 1;
            if words.get(at).is_some_and(|&j| tokens[j] == "[")
                && let Some(p) = words.iter().position(|&x| Some(x) == matching(words[at]))
            {
                at = p + 1;
            }
            let mut names = Vec::new();
            let mut depth = 0;
            let mut expect_name = true;
            for &j in &words[at.min(words.len())..] {
                match tokens[j] {
                    "[" | "(" => depth += 1,
                    "]" | ")" => depth -= 1,
                    "," if depth == 0 => expect_name = true,
                    t if depth == 0 && expect_name && is_ident(t) => {
                        names.push((j, t));
                        expect_name = false;
                    }
                    _ => {}
                }
            }
            for &(j, name) in &names {
                if vector {
                    fail(
                        j,
                        format!(
                            "buffer member `{name}`: a `{}` is not supported in a storage buffer",
                            tokens[ty]
                        ),
                    );
                } else {
                    members.push(Member { type_at: ty, name });
                }
            }
        }
        if members.is_empty() {
            continue;
        }
        if instance.is_some_and(|j| next(j).is_some_and(|n| tokens[n] == "[")) {
            for m in &members {
                fail(
                    m.type_at,
                    format!(
                        "buffer member `{}`: an instance array of a buffer with `bool` members is not supported",
                        m.name
                    ),
                );
            }
            continue;
        }
        blocks.push(Block {
            open,
            close,
            instance: instance.map(|j| tokens[j]),
            members,
        });
    }
    if blocks.is_empty() {
        return (None, errors);
    }

    // Texts put before and after tokens.
    let mut before: BTreeMap<usize, String> = BTreeMap::new();
    let mut after: BTreeMap<usize, String> = BTreeMap::new();
    let mut retyped: Vec<usize> = Vec::new();
    for block in &blocks {
        retyped.extend(block.members.iter().map(|m| m.type_at));
    }
    // The uses of each member.
    let kind = |i: usize| tokens[i];
    for block in &blocks {
        for member in &block.members {
            for (i, &t) in tokens.iter().enumerate() {
                if t != member.name || (i > block.open && i < block.close) {
                    continue;
                }
                // `instance . name`, or the bare name for a block without an instance.
                let (start, name_at) = match block.instance {
                    Some(instance) => {
                        let dot = prev(i).filter(|&d| kind(d) == ".");
                        let owner = dot.and_then(prev).filter(|&o| kind(o) == instance);
                        match owner {
                            Some(o) => (o, i),
                            None => continue,
                        }
                    }
                    None => {
                        if prev(i).is_some_and(|d| kind(d) == ".") {
                            continue; // Another value's member of the same name.
                        }
                        // A declaration of the same name (a local, a parameter) hides it.
                        if prev(i).is_some_and(|d| {
                            is_ident(kind(d))
                                && !matches!(
                                    kind(d),
                                    "return" | "else" | "case" | "do" | "discard" | "in" | "out"
                                )
                        }) {
                            fail(
                                i,
                                format!(
                                    "buffer member `{}` is also declared as a local or parameter; \
                                     rename one of them",
                                    member.name
                                ),
                            );
                            continue;
                        }
                        (i, i)
                    }
                };
                // The reference ends after its `[...]` groups.
                let mut last = name_at;
                let mut following = next(last);
                while let Some(open) = following.filter(|&j| kind(j) == "[") {
                    match matching(open) {
                        Some(close) => {
                            last = close;
                            following = next(close);
                        }
                        None => break,
                    }
                }
                let op = following.map(kind);
                let adjacent = |j: usize| tokens.get(j + 1).copied();
                match (following, op) {
                    // `ref = value;` as a statement.
                    (Some(eq), Some("=")) if adjacent(eq) != Some("=") => {
                        let statement = prev(start)
                            .is_none_or(|p| matches!(kind(p), ";" | "{" | "}" | ")" | "else"));
                        let mut depth = 0i32;
                        let end = (eq + 1..tokens.len()).find(|&j| {
                            match tokens[j] {
                                "(" | "[" | "{" => depth += 1,
                                ")" | "]" | "}" => depth -= 1,
                                _ => {}
                            }
                            depth < 0 || (depth == 0 && tokens[j] == ";")
                        });
                        match (statement, end) {
                            (true, Some(end)) => {
                                let first = next(eq).unwrap_or(eq + 1);
                                before.entry(first).or_default().push_str("uint(");
                                before.entry(end).or_default().insert(0, ')');
                            }
                            _ => fail(
                                i,
                                format!(
                                    "buffer member `{}` is assigned inside an expression; \
                                     assign it in a statement of its own",
                                    member.name
                                ),
                            ),
                        }
                    }
                    // `ref |= value;` and the like.
                    (Some(op_at), Some("|" | "&" | "^" | "+" | "-" | "*" | "/" | "%"))
                        if adjacent(op_at) == Some("=") =>
                    {
                        fail(
                            i,
                            format!(
                                "buffer member `{}`: `{}=` on a `bool` in a storage buffer is not \
                                 supported; write `{} = {} {} ...;`",
                                member.name,
                                kind(op_at),
                                member.name,
                                member.name,
                                match kind(op_at) {
                                    "|" => "||",
                                    "&" => "&&",
                                    other => other,
                                }
                            ),
                        );
                    }
                    // A read.
                    _ => {
                        before.entry(start).or_default().push('(');
                        after.entry(last).or_default().push_str(" != 0u)");
                    }
                }
            }
        }
    }
    if !errors.is_empty() {
        return (None, errors);
    }
    let mut code = String::with_capacity(pp.code.len() + 32);
    for (i, &t) in tokens.iter().enumerate() {
        if let Some(text) = before.get(&i) {
            code.push_str(text);
        }
        if retyped.contains(&i) {
            code.push_str("uint");
        } else {
            code.push_str(t);
        }
        if let Some(text) = after.get(&i) {
            code.push_str(text);
        }
    }
    (Some(Preprocessed { code, ..pp.clone() }), errors)
}

#[cfg(test)]
mod tests {
    use super::rewrite;
    use crate::pack::tests::zip;
    use crate::{Macros, Pack, preprocess};

    /// The rewritten code of a fragment stage, `None` when nothing changes.
    fn code(source: &str) -> Option<String> {
        let pack =
            Pack::from_zip_bytes("P.zip", zip(&[("shaders/composite.fsh", source)])).unwrap();
        let stage = preprocess(&pack, "composite.fsh", &Macros::standard());
        let (rewritten, errors) = rewrite(&stage);
        assert!(errors.is_empty(), "{errors:?}");
        rewritten.map(|p| p.code)
    }

    #[test]
    fn reads_become_comparisons_and_plain_assignments_convert_back() {
        const SOURCE: &str = "#version 430\nlayout(std430, binding = 4) buffer State { mat4 matrix; bool flag; };\n\
             void main() {\n    if (flag) { }\n    flag = !flag && true;\n    bool seen = flag;\n}\n";
        let out = code(SOURCE).unwrap();
        assert!(out.contains("mat4 matrix; uint flag;"), "{out}");
        assert!(out.contains("if ((flag != 0u))"), "{out}");
        assert!(out.contains("flag = uint(!(flag != 0u) && true);"), "{out}");
        assert!(out.contains("bool seen = (flag != 0u);"), "{out}");
        // The rewrite adds no line and takes none: the pack's line numbers stay right.
        let pack =
            Pack::from_zip_bytes("P.zip", zip(&[("shaders/composite.fsh", SOURCE)])).unwrap();
        let before = preprocess(&pack, "composite.fsh", &Macros::standard());
        assert_eq!(out.lines().count(), before.code.lines().count());
    }

    #[test]
    fn instances_and_arrays_are_rewritten_where_they_are_read() {
        let out = code(
            "#version 430\nlayout(std430, binding = 4) buffer State { int n; bool flags[4]; bool on; } s;\n\
             void main() {\n    float a = s.on ? 1.0 : 0.0;\n    bool b = s.flags[2] && !s.flags[s.n];\n}\n",
        )
        .unwrap();
        assert!(out.contains("uint flags[4]; uint on;"), "{out}");
        assert!(out.contains("(s.on != 0u) ? 1.0"), "{out}");
        assert!(
            out.contains("(s.flags[2] != 0u) && !(s.flags[s.n] != 0u)"),
            "{out}"
        );
    }

    #[test]
    fn nothing_to_do_without_a_bool_member_and_a_same_named_field_is_refused() {
        // No bool in a buffer: nothing to do.
        assert!(
            code(
                "#version 430
layout(std430, binding = 4) buffer B { int n; };
bool flag;
void main() { }
"
            )
            .is_none()
        );
        // Another value's member of the same name cannot be told from the buffer's by tokens
        // alone, so it is refused with the name, not rewritten wrongly.
        let pack = Pack::from_zip_bytes(
            "P.zip",
            zip(&[(
                "shaders/composite.fsh",
                "#version 430
struct Light { bool flag; };
layout(std430, binding = 4) buffer B { bool flag; };
                 void main() {
    Light light;
    bool a = light.flag;
}
",
            )]),
        )
        .unwrap();
        let (rewritten, errors) = rewrite(&preprocess(&pack, "composite.fsh", &Macros::standard()));
        assert!(rewritten.is_none());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].message.contains("`flag` is also declared"));
        assert_eq!(errors[0].line, 2);
    }
}
