//! PURPOSE: convert each argument of a scalar or vector array constructor to the element type.
//! WHY: GLSL drivers take `int[2](1u, 2)`, `float[](1u, 2.0)`; naga's frontend converts arguments
//! only upward and builds a `Compose` whose first component has another scalar type, which its
//! validator refuses ("Composing 0's component type is not expected", Horizon #7168).
//! `T(x)` of an `x` already a `T` is the identity, so correct sources do not change.
//! REFS: GLSL 4.60 section 5.4.4 (array constructors).
//! LOGS: none.
//! READS-BY: `compile` in `mod.rs`, before parsing.
//! ENDS-WHEN: naga's GLSL frontend converts array constructor arguments itself.
use crate::macros::lex;

const ELEMENTS: [&str; 15] = [
    "float", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2", "ivec3", "ivec4", "uvec2",
    "uvec3", "uvec4", "bvec2", "bvec3",
];

pub(super) fn rewrite(text: &str) -> String {
    let tokens = lex(text);
    walk(&tokens)
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

fn walk(tokens: &[&str]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(close) = constructor_end(tokens, i) {
            // tokens[i] is the element type; find the `(` after `]`.
            let open = (i..close)
                .find(|&k| tokens[k] == "(")
                .expect("constructor has a paren");
            for t in &tokens[i..=open] {
                out.push_str(t);
            }
            let (mut depth, mut start) = (0usize, open + 1);
            for k in open + 1..=close {
                let t = tokens[k];
                match t {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" if depth > 0 => depth -= 1,
                    "," | ")" if depth == 0 => {
                        push_arg(&mut out, tokens[i], &tokens[start..k]);
                        out.push_str(t);
                        start = k + 1;
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
fn push_arg(out: &mut String, ty: &str, arg: &[&str]) {
    let lead = arg.iter().take_while(|t| is_ws(t)).count();
    let trail = arg[lead..].iter().rev().take_while(|t| is_ws(t)).count();
    let body = &arg[lead..arg.len() - trail];
    arg[..lead].iter().for_each(|t| out.push_str(t));
    if !body.is_empty() {
        out.push_str(ty);
        out.push('(');
        out.push_str(&walk(body));
        out.push(')');
    }
    arg[arg.len() - trail..].iter().for_each(|t| out.push_str(t));
}

/// If an array constructor `T[N](...)` of a scalar or vector `T` starts at `i`, the index of its `)`.
fn constructor_end(tokens: &[&str], i: usize) -> Option<usize> {
    if !ELEMENTS.contains(&tokens[i]) {
        return None;
    }
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
    let mut depth = 0usize;
    for (j, t) in tokens.iter().enumerate().skip(k) {
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
}
