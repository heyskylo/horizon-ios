//! PURPOSE: convert each argument of an array constructor, or a struct constructor, whose elements
//! or members are scalars or vectors, to the element or member type.
//! WHY: GLSL drivers take `int[2](1u, 2)`, `S(1u, 2.0)`; naga's frontend converts arguments
//! only upward and builds a `Compose` whose first component has another scalar type, which its
//! validator refuses ("Composing 0's component type is not expected", Horizon #7168).
//! `T(x)` of an `x` already a `T` is the identity, so correct sources do not change.
//! REFS: GLSL 4.60 sections 5.4.4 (array constructors) and 5.4.3 (structure constructors).
//! LOGS: none.
//! READS-BY: `compile` in `mod.rs`, before parsing.
//! ENDS-WHEN: naga's GLSL frontend converts array and struct constructor arguments itself.
use crate::macros::lex;

const ELEMENTS: [&str; 15] = [
    "float", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2", "ivec3", "ivec4", "uvec2",
    "uvec3", "uvec4", "bvec2", "bvec3",
];

pub(super) fn rewrite(text: &str) -> String {
    let tokens = lex(text);
    let structs = structs(&tokens);
    walk(&tokens, &structs)
}

type Structs<'a> = Vec<(&'a str, Vec<&'a str>)>;

/// Each `struct S { ... };` whose members are all scalars or vectors: its name and member types.
fn structs<'a>(tokens: &[&'a str]) -> Structs<'a> {
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        if tokens[i] != "struct" {
            continue;
        }
        let (Some(n), Some(o)) = (
            next(tokens, i + 1),
            next(tokens, i + 1).and_then(|n| next(tokens, n + 1)),
        ) else {
            continue;
        };
        if tokens[o] != "{" {
            continue;
        }
        let Some(end) = tokens[o..].iter().position(|t| *t == "}") else {
            continue;
        };
        let mut members = Vec::new();
        let mut ok = true;
        for decl in tokens[o + 1..o + end].split(|t| *t == ";") {
            let words: Vec<&str> = decl.iter().copied().filter(|t| !is_ws(t)).collect();
            let Some((ty, names)) = words.split_first() else {
                continue;
            };
            if !ELEMENTS.contains(ty) || names.contains(&"[") {
                ok = false;
                break;
            }
            members.extend(names.split(|t| *t == ",").map(|_| *ty));
        }
        if ok && !members.is_empty() {
            out.push((tokens[n], members));
        }
    }
    out
}

fn is_ws(t: &str) -> bool {
    t.trim().is_empty()
}

/// The next significant token at or after `i`.
fn next(tokens: &[&str], mut i: usize) -> Option<usize> {
    while i < tokens.len() && is_ws(tokens[i]) {
        i += 1;
    }
    (i < tokens.len()).then_some(i)
}

fn walk(tokens: &[&str], structs: &Structs) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((close, types)) = constructor_end(tokens, i, structs) {
            // tokens[i] is the element or struct type; find the `(` after it (and its `]`).
            let open = (i..close)
                .find(|&k| tokens[k] == "(")
                .expect("constructor has a paren");
            for t in &tokens[i..=open] {
                out.push_str(t);
            }
            let (mut depth, mut start, mut arg) = (0usize, open + 1, 0usize);
            for k in open + 1..=close {
                let t = tokens[k];
                match t {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" if depth > 0 => depth -= 1,
                    "," | ")" if depth == 0 => {
                        let ty = types
                            .get(arg)
                            .or(types.last())
                            .copied()
                            .unwrap_or(tokens[i]);
                        push_arg(&mut out, ty, &tokens[start..k], structs);
                        out.push_str(t);
                        start = k + 1;
                        arg += 1;
                    }
                    _ => {}
                }
            }
            i = close + 1;
        } else {
            out.push_str(tokens[i]);
            i += 1;
        }
    }
    out
}

/// `T(arg)` around one argument, its white space left outside; nested constructors rewritten.
fn push_arg(out: &mut String, ty: &str, arg: &[&str], structs: &Structs) {
    let lead = arg.iter().take_while(|t| is_ws(t)).count();
    let trail = arg[lead..].iter().rev().take_while(|t| is_ws(t)).count();
    let body = &arg[lead..arg.len() - trail];
    arg[..lead].iter().for_each(|t| out.push_str(t));
    if !body.is_empty() {
        out.push_str(ty);
        out.push('(');
        out.push_str(&walk(body, structs));
        out.push(')');
    }
    arg[arg.len() - trail..]
        .iter()
        .for_each(|t| out.push_str(t));
}

/// If an array constructor `T[N](...)` of a scalar or vector `T`, or a constructor of a struct
/// with that many scalar or vector members, starts at `i`: the index of its `)` and the type
/// each argument takes (one type for an array; a struct's, in order, when its count matches).
fn constructor_end<'a>(
    tokens: &[&str],
    i: usize,
    structs: &Structs<'a>,
) -> Option<(usize, Vec<&'a str>)> {
    if let Some((_, members)) = structs.iter().find(|(n, _)| *n == tokens[i]) {
        let open = next(tokens, i + 1).filter(|&k| tokens[k] == "(")?;
        let close = matching(tokens, open)?;
        let args = 1 + tokens[open + 1..close]
            .iter()
            .scan(0i32, |d, t| {
                match *t {
                    "(" | "[" | "{" => *d += 1,
                    ")" | "]" | "}" => *d -= 1,
                    _ => {}
                }
                Some(if *d == 0 && *t == "," { 1 } else { 0 })
            })
            .sum::<usize>();
        return (args == members.len()).then(|| (close, members.clone()));
    }
    let elem = ELEMENTS.iter().find(|e| **e == tokens[i])?;
    let mut k = next(tokens, i + 1)?;
    if tokens[k] != "[" {
        return None;
    }
    k = next(tokens, k + 1)?;
    if tokens[k] != "]" {
        k = next(tokens, k + 1)?;
        if tokens[k] != "]" {
            return None;
        }
    }
    k = next(tokens, k + 1)?;
    if tokens[k] != "(" {
        return None;
    }
    Some((matching(tokens, k)?, vec![*elem]))
}

/// The index of the bracket closing the one at `open`.
fn matching(tokens: &[&str], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, t) in tokens.iter().enumerate().skip(open) {
        match *t {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_take_the_element_type_and_lines_stay() {
        let src = "int k[2] = int[2](1u,\n 2);\nvec2 v[] = vec2[](a, vec2(1, 2));\nfloat f(int x) { return float[](x, 1.0)[0]; }";
        assert_eq!(
            rewrite(src),
            "int k[2] = int[2](int(1u),\n int(2));\nvec2 v[] = vec2[](vec2(a), vec2(vec2(1, 2)));\nfloat f(int x) { return float[](float(x), float(1.0))[0]; }"
        );
    }

    #[test]
    fn struct_arguments_take_the_member_types() {
        let src = "struct S { int a; float b, c; };\nS s = S(1u, 2,\n x);";
        assert_eq!(
            rewrite(src),
            "struct S { int a; float b, c; };\nS s = S(int(1u), float(2),\n float(x));"
        );
    }
}
