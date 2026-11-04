//! Atomic functions on a component of an integer vector in shared memory or in a buffer block
//! (`atomicAdd(sums[i], n)`, `atomicMax(lights[k].w, w)`, `atomicAdd(buf.counts.x, 1u)`): GLSL
//! allows them (GLSL 4.60, 8.11), naga has no atomic vectors. Such a vector (or array of vectors)
//! becomes an array of its components, `shared int sums_hz[4]`, `uint counts_hz[2];` in the
//! block; a component is an element, a whole vector is read through `hz_get_sums()` and written
//! through `hz_set_sums(v)`. A `std430` buffer block keeps its layout: a padding member goes
//! before the array when the vector's alignment would have moved it, and an array of 3-component
//! vectors keeps a fourth, unused element per vector (its stride). Other layouts, runtime-sized
//! arrays and blocks whose layout is not known up to the vector stay as written. The source
//! keeps its lines.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::scan::{is_ident, significant};
use crate::Preprocessed;
use crate::macros::lex;

const ATOMICS: [&str; 8] = [
    "atomicAdd",
    "atomicMin",
    "atomicMax",
    "atomicAnd",
    "atomicOr",
    "atomicXor",
    "atomicExchange",
    "atomicCompSwap",
];

/// A vector rewritten: its type, size, array length if any, the elements'
/// name (`sums_hz`, `buf.counts_hz`) and the name of its getter and setter (`hz_get_<key>`).
struct Split {
    vector: String,
    size: usize,
    array: Option<String>,
    base: String,
    key: String,
}

/// How a rewritten vector is named where it is used: a name of its own (a shared variable, a
/// member of a block without an instance name), or `instance.member`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Path<'a> {
    Name(&'a str),
    Member(&'a str, &'a str),
}

/// `ivecN`/`uvecN`: the scalar type and N.
fn integer_vector(ty: &str) -> Option<(&'static str, usize)> {
    let (scalar, n) = match ty {
        v if v.starts_with("ivec") => ("int", &v[4..]),
        v if v.starts_with("uvec") => ("uint", &v[4..]),
        _ => return None,
    };
    n.parse()
        .ok()
        .filter(|n| (2..=4).contains(n))
        .map(|n| (scalar, n))
}

/// A member type's std430 base alignment and size in bytes (the rules Horizon's naga applies in
/// `front/glsl/offset.rs`), for the scalars, vectors and float matrices; `None` for any other
/// type.
fn std430(ty: &str) -> Option<(u32, u32)> {
    let vector = |n: u32| match n {
        2 => Some((8, 8)),
        3 => Some((16, 12)),
        4 => Some((16, 16)),
        _ => None,
    };
    match ty {
        "int" | "uint" | "float" | "bool" => Some((4, 4)),
        _ if ty.starts_with("mat") => {
            let dims = &ty[3..];
            let (columns, rows) = match dims.split_once('x') {
                Some((c, r)) => (c.parse().ok()?, r.parse().ok()?),
                None => (dims.parse().ok()?, dims.parse().ok()?),
            };
            let (align, _) = vector(rows)?;
            vector(columns)?;
            Some((align, align * columns))
        }
        _ => {
            let n = ty
                .strip_prefix(['i', 'u', 'b'])
                .unwrap_or(ty)
                .strip_prefix("vec")?;
            vector(n.parse().ok()?)
        }
    }
}

/// `pp` with its shared and buffer vectors that atomic functions use by component rewritten.
pub(super) fn rewrite(pp: &Preprocessed) -> Option<Preprocessed> {
    if !ATOMICS.iter().any(|a| pp.code.contains(a)) {
        return None;
    }
    let tokens = lex(&pp.code);
    let next = |i: usize| (i + 1..tokens.len()).find(|&j| significant(tokens[j]));
    let prev = |i: usize| (0..i).rev().find(|&j| significant(tokens[j]));
    // `[N]` at `open`: its size's text and the token after `]`.
    let bracket = |open: usize| -> Option<(String, Option<usize>)> {
        let close = (open..tokens.len()).find(|&j| tokens[j] == "]")?;
        let size: String = tokens[open + 1..close].concat();
        Some((size.trim().to_owned(), next(close)))
    };
    // What an atomic function takes a component of: its first name, and the name after a `.`.
    let mut used: Vec<(&str, Option<&str>)> = Vec::new();
    for (i, &t) in tokens.iter().enumerate() {
        if ATOMICS.contains(&t)
            && let Some(open) = next(i).filter(|&j| tokens[j] == "(")
            && let Some(name) = next(open).filter(|&j| is_ident(tokens[j]))
        {
            let member = next(name)
                .filter(|&j| tokens[j] == ".")
                .and_then(next)
                .filter(|&j| is_ident(tokens[j]))
                .map(|j| tokens[j]);
            used.push((tokens[name], member));
        }
    }
    let mut splits: BTreeMap<Path, Split> = BTreeMap::new();
    // Declarations replaced (tokens `start..=end` by the text), and text after a token.
    let mut decls: Vec<(usize, usize, String)> = Vec::new();
    let mut after: Vec<(usize, String)> = Vec::new();
    // Shared declarations: `shared ivec4 name;`, `shared ivec4[N] name;`, `shared ivec4 name[N];`.
    for (i, &t) in tokens.iter().enumerate() {
        if t != "shared" {
            continue;
        }
        let Some(ty) = next(i) else { continue };
        let Some((scalar, size)) = integer_vector(tokens[ty]) else {
            continue;
        };
        let mut j = next(ty);
        let mut array = None;
        if let Some((size, after)) = j.filter(|&j| tokens[j] == "[").and_then(bracket) {
            array = Some(size);
            j = after;
        }
        let Some(name_at) = j.filter(|&j| is_ident(tokens[j])) else {
            continue;
        };
        let name = tokens[name_at];
        let mut end = next(name_at);
        if let Some((size, after)) = end.filter(|&e| tokens[e] == "[").and_then(bracket) {
            array = Some(size);
            end = after;
        }
        let Some(end) = end.filter(|&e| tokens[e] == ";") else {
            continue;
        };
        if !used.iter().any(|u| u.0 == name) {
            continue;
        }
        let elements = match &array {
            None => format!("{name}_hz[{size}]"),
            Some(len) => format!("{name}_hz[{len}][{size}]"),
        };
        let split = Split {
            vector: tokens[ty].to_owned(),
            size,
            array,
            base: format!("{name}_hz"),
            key: name.to_owned(),
        };
        let decl = format!("shared {scalar} {elements}; {}", accessors(&split));
        decls.push((i, end, decl));
        splits.insert(Path::Name(name), split);
    }
    // Buffer blocks: `layout(std430, ...) buffer Name { members } instance;`.
    for (i, &t) in tokens.iter().enumerate() {
        if t != "buffer" {
            continue;
        }
        let Some(open) = next(i)
            .filter(|&j| is_ident(tokens[j]))
            .and_then(next)
            .filter(|&j| tokens[j] == "{")
        else {
            continue;
        };
        // The declaration's qualifiers, back to the previous declaration.
        let first = (0..i)
            .rev()
            .find(|&j| matches!(tokens[j], ";" | "}"))
            .map_or(0, |j| j + 1);
        if !tokens[first..i].contains(&"std430") {
            continue;
        }
        let Some(close) = (open..tokens.len()).find(|&j| tokens[j] == "}") else {
            continue;
        };
        let (instance, end) = match next(close) {
            Some(j) if tokens[j] == ";" => (None, j),
            Some(j) if is_ident(tokens[j]) && next(j).is_some_and(|k| tokens[k] == ";") => {
                (Some(tokens[j]), next(j).unwrap())
            }
            _ => continue,
        };
        // The members in order, with their std430 offsets while they are known.
        let mut offset = Some(0u32);
        let mut from = open + 1;
        for semi in (open + 1..close).filter(|&j| tokens[j] == ";") {
            let words: Vec<usize> = (from..semi).filter(|&j| significant(tokens[j])).collect();
            from = semi + 1;
            let words: Vec<usize> = words
                .into_iter()
                .skip_while(|&j| {
                    matches!(
                        tokens[j],
                        "highp"
                            | "mediump"
                            | "lowp"
                            | "coherent"
                            | "volatile"
                            | "restrict"
                            | "readonly"
                            | "writeonly"
                            | "precise"
                    )
                })
                .collect();
            let [ty, name_at, ref rest @ ..] = words[..] else {
                offset = None;
                continue;
            };
            let (ty, name) = (tokens[ty], tokens[name_at]);
            // One declarator, with at most one array size. A runtime-sized array (`[]`, last)
            // stays as written: naga rejected the array of arrays it would become.
            let array = match rest.iter().map(|&j| tokens[j]).collect::<Vec<_>>()[..] {
                [] => None,
                ["[", ref len @ .., "]"]
                    if !len.is_empty() && !len.contains(&"[") && !len.contains(&",") =>
                {
                    Some(len.concat())
                }
                _ => {
                    offset = None;
                    continue;
                }
            };
            let layout = std430(ty);
            let end_before = offset;
            let at = offset
                .zip(layout)
                .map(|(o, (align, _))| o.next_multiple_of(align));
            offset = at.zip(layout).and_then(|(at, (align, size))| match &array {
                None => Some(at + size),
                Some(len) => Some(at + len.parse::<u32>().ok()? * size.next_multiple_of(align)),
            });
            let Some((scalar, size)) = integer_vector(ty) else {
                continue;
            };
            let path = match instance {
                Some(instance) => Path::Member(instance, name),
                None => Path::Name(name),
            };
            let is_used = used.iter().any(|&u| match path {
                Path::Member(instance, member) => u == (instance, Some(member)),
                Path::Name(name) => u.0 == name,
            });
            let Some(at) = at.filter(|_| is_used) else {
                continue;
            };
            // The bytes the vector's alignment skips, which the elements' does not.
            let pad = end_before.map_or(0, |e| (at - e) / 4);
            let mut decl = String::new();
            if pad > 0 {
                write!(decl, "uint hz_pad_{name}[{pad}]; ").unwrap();
            }
            // In an array, a 3-component vector takes 16 bytes: four elements.
            let width = if array.is_some() && size == 3 {
                4
            } else {
                size
            };
            match &array {
                None => write!(decl, "{scalar} {name}_hz[{width}];"),
                Some(len) => write!(decl, "{scalar} {name}_hz[{len}][{width}];"),
            }
            .unwrap();
            let base = match instance {
                Some(instance) => format!("{instance}.{name}_hz"),
                None => format!("{name}_hz"),
            };
            let key = match instance {
                Some(instance) => format!("{instance}_{name}"),
                None => name.to_owned(),
            };
            let split = Split {
                vector: ty.to_owned(),
                size,
                array,
                base,
                key,
            };
            after.push((end, format!(" {}", accessors(&split))));
            decls.push((words[0], semi, decl));
            splits.insert(path, split);
        }
    }
    if splits.is_empty() {
        return None;
    }
    let mut out: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
    for (start, end, decl) in &decls {
        for t in &mut out[*start..=*end] {
            *t = t.chars().filter(|&c| c == '\n').collect();
        }
        out[*start] = decl.clone() + &out[*start];
    }
    for (at, text) in &after {
        out[*at].push_str(text);
    }
    let component = |s: &str| match s {
        "x" | "r" | "s" => Some(0),
        "y" | "g" | "t" => Some(1),
        "z" | "b" | "p" => Some(2),
        "w" | "a" | "q" => Some(3),
        _ => None,
    };
    // Uses: from the name, past `[k]` for an array, then a component, a swizzle, an assignment
    // or the whole vector.
    let mut i = 0;
    while i < tokens.len() {
        let in_decl = decls.iter().any(|&(a, b, _)| (a..=b).contains(&i));
        let member = next(i)
            .filter(|&j| tokens[j] == ".")
            .and_then(next)
            .filter(|&j| is_ident(tokens[j]));
        let found = if in_decl || prev(i).is_some_and(|p| tokens[p] == ".") {
            None
        } else if let Some(m) = member
            && let Some(s) = splits.get(&Path::Member(tokens[i], tokens[m]))
        {
            Some((s, m))
        } else {
            splits.get(&Path::Name(tokens[i])).map(|s| (s, i))
        };
        let Some((s, name_end)) = found else {
            i += 1;
            continue;
        };
        let key = &s.key;
        let mut base = s.base.clone();
        let mut getter_args = String::new();
        let mut last = name_end;
        if s.array.is_some() {
            // `name.length()`: the array's own.
            if next(name_end).is_some_and(|j| tokens[j] == ".") {
                out[i] = base;
                for t in &mut out[i + 1..=name_end] {
                    *t = t.chars().filter(|&c| c == '\n').collect();
                }
                i = name_end + 1;
                continue;
            }
            // `name[k]`: the index's tokens, written back as they are.
            let Some(open) = next(name_end).filter(|&j| tokens[j] == "[") else {
                i += 1;
                continue;
            };
            let mut depth = 0;
            let mut close = open;
            for (j, &t) in tokens.iter().enumerate().skip(open) {
                match t {
                    "[" => depth += 1,
                    "]" => {
                        depth -= 1;
                        if depth == 0 {
                            close = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let index: String = tokens[open + 1..close].concat();
            base = format!("{base}[{index}]");
            // GLSL won't turn a `uint` index into the `int` the getter and setter take.
            getter_args = format!("int({index})");
            last = close;
        }
        let after = next(last);
        let follows = |t: &str| after.is_some_and(|j| tokens[j] == t);
        let assigned = |at: Option<usize>| {
            at.is_some_and(|j| tokens[j] == "=" && next(j).is_none_or(|k| tokens[k] != "="))
        };
        // `name op= value;`: the operator's tokens and the `=`.
        let compound = after.and_then(|op| {
            let first = tokens[op];
            let second = next(op)?;
            match (first, tokens[second]) {
                ("+" | "-" | "*" | "/" | "%" | "&" | "|" | "^", "=") => {
                    Some((first.to_owned(), second))
                }
                ("<" | ">", t) if t == first => {
                    let eq = next(second).filter(|&k| tokens[k] == "=")?;
                    Some((format!("{first}{first}"), eq))
                }
                _ => None,
            }
        });
        let replacement;
        let mut end = last;
        if let Some((op, eq)) = compound {
            // `name op= value;` → `hz_set_name(hz_get_name() op (value));`
            let semi = (eq..tokens.len()).find(|&j| tokens[j] == ";").unwrap_or(eq);
            let args = if getter_args.is_empty() {
                String::new()
            } else {
                format!("{getter_args}, ")
            };
            out[i] = format!("hz_set_{key}({args}hz_get_{key}({getter_args}) {op} (");
            for t in &mut out[i + 1..=eq] {
                *t = t.chars().filter(|&c| c == '\n').collect();
            }
            out[semi] = "));".into();
            i = eq + 1;
            continue;
        } else if follows(".") || (s.array.is_none() && follows("[")) {
            let at = after.and_then(next).unwrap();
            if follows("[") {
                // `name[c]`: an element already.
                replacement = base.clone();
            } else if let Some(c) = component(tokens[at]) {
                replacement = format!("{base}[{c}]");
                end = at;
            } else {
                // A swizzle of several components: read only.
                let get = format!("hz_get_{key}({getter_args})");
                replacement = format!("{get}.{}", tokens[at]);
                end = at;
            }
        } else if assigned(after) {
            // `name = value;` → `hz_set_name(value);`
            let eq = after.unwrap();
            let semi = (eq..tokens.len()).find(|&j| tokens[j] == ";").unwrap_or(eq);
            let args = if getter_args.is_empty() {
                String::new()
            } else {
                format!("{getter_args}, ")
            };
            out[i] = format!("hz_set_{key}({args}");
            for t in &mut out[i + 1..=eq] {
                *t = t.chars().filter(|&c| c == '\n').collect();
            }
            out[semi] = ");".into();
            i = eq + 1;
            continue;
        } else {
            replacement = format!("hz_get_{key}({getter_args})");
        }
        out[i] = replacement;
        for t in &mut out[i + 1..=end] {
            *t = t.chars().filter(|&c| c == '\n').collect();
        }
        i = end + 1;
    }
    Some(Preprocessed {
        code: out.concat(),
        lines: pp.lines.clone(),
        files: pp.files.clone(),
        version: pp.version.clone(),
        extensions: pp.extensions.clone(),
        comments: pp.comments.clone(),
        diagnostics: pp.diagnostics.clone(),
        warnings: pp.warnings.clone(),
    })
}

/// `hz_get_<key>` and `hz_set_<key>`: a whole vector of `split` (of an array: the `k`th),
/// read from and written to its elements.
fn accessors(split: &Split) -> String {
    let (v, base, key) = (&split.vector, &split.base, &split.key);
    let element = if split.array.is_some() {
        format!("{base}[k]")
    } else {
        base.clone()
    };
    let comps = (0..split.size)
        .map(|c| format!("{element}[{c}]"))
        .collect::<Vec<_>>()
        .join(", ");
    let sets: String = (0..split.size)
        .map(|c| format!("{element}[{c}] = v[{c}];"))
        .collect();
    let (k, k_comma) = if split.array.is_some() {
        ("int k", "int k, ")
    } else {
        ("", "")
    };
    format!(
        "{v} hz_get_{key}({k}) {{ return {v}({comps}); }} void hz_set_{key}({k_comma}{v} v) {{ {sets} }}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewritten(code: &str) -> String {
        let pp = Preprocessed {
            code: code.to_owned(),
            lines: Vec::new(),
            files: Vec::new(),
            version: None,
            extensions: Vec::new(),
            comments: Vec::new(),
            diagnostics: Vec::new(),
            warnings: Vec::new(),
        };
        let out = rewrite(&pp).expect("rewritten").code;
        assert_eq!(out.lines().count(), code.lines().count(), "{out}");
        out
    }

    #[test]
    fn components_whole_vectors_and_compound_assignments() {
        let out = rewritten(
            "shared ivec4 sums;\nvoid f(ivec4 v) {\n atomicAdd(sums.y, 1);\n ivec4 a = sums;\n \
             sums = v;\n sums += v;\n sums <<= 1;\n int z = sums.z;\n ivec2 xy = sums.xy;\n}\n",
        );
        assert!(out.contains("shared int sums_hz[4];"));
        assert!(out.contains("atomicAdd(sums_hz[1], 1);"));
        assert!(out.contains("ivec4 a = hz_get_sums();"));
        assert!(out.contains("hz_set_sums( v);"));
        assert!(out.contains("hz_set_sums(hz_get_sums() + ( v));"));
        assert!(out.contains("hz_set_sums(hz_get_sums() << ( 1));"));
        assert!(out.contains("int z = sums_hz[2];"));
        assert!(out.contains("ivec2 xy = hz_get_sums().xy;"));
    }

    #[test]
    fn arrays_of_vectors_with_any_size() {
        let out = rewritten(
            "#define N 4\nshared uvec2 lights[N * 2];\nvoid f(int k) {\n atomicMax(lights[k + 1].x, 3u);\n \
             lights[k] += uvec2(1u);\n uvec2 l = lights[k];\n}\n",
        );
        assert!(out.contains("shared uint lights_hz[N * 2][2];"), "{out}");
        assert!(out.contains("atomicMax(lights_hz[k + 1][0], 3u);"));
        assert!(out.contains("hz_set_lights(int(k), hz_get_lights(int(k)) + ( uvec2(1u)));"));
        assert!(out.contains("uvec2 l = hz_get_lights(int(k));"));
    }

    #[test]
    fn untouched_without_atomics_on_a_component() {
        let pp = |code: &str| Preprocessed {
            code: code.to_owned(),
            lines: Vec::new(),
            files: Vec::new(),
            version: None,
            extensions: Vec::new(),
            comments: Vec::new(),
            diagnostics: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(rewrite(&pp("shared ivec4 v;\nvoid f() { v.x = 1; }\n")).is_none());
        assert!(rewrite(&pp("shared int n;\nvoid f() { atomicAdd(n, 1); }\n")).is_none());
    }
}
