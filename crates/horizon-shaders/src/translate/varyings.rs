//! The varyings between a program's stages: which travel, at which location, and in what parts.

use std::collections::BTreeMap;

use super::Error;
use super::scan::Stage;

/// The varyings of a program: by name, its location and declaration (`@` for the name); the
/// matrices, arrays and structs that travel in parts (their global, and each part with how to
/// reach it); and the vertex outputs that stay globals of the vertex stage.
pub(super) struct Varyings {
    pub locations: BTreeMap<String, (u32, String)>,
    pub split: BTreeMap<String, (String, Vec<(String, String)>)>,
    pub private: Vec<String>,
}

/// Varyings by name: the vertex outputs the fragment stage reads, then fragment inputs the vertex
/// stage lacks. A matrix or array travels as one varying per column or element
/// (`varying_parts`), a struct as its members packed into vectors (`struct_parts`).
pub(super) fn assign(v: &Stage, f: &Stage, errors: &mut Vec<Error>) -> Varyings {
    let mut locations: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut split: BTreeMap<String, (String, Vec<(String, String)>)> = BTreeMap::new();
    let mut private = Vec::new();
    let vars = v.outs.iter().map(|o| (v, o));
    for (stage, var) in vars.chain(f.ins.iter().map(|i| (f, i))) {
        if locations.contains_key(&var.name) || split.contains_key(&var.name) {
            continue;
        }
        // A pack's varying its fragment stage never reads takes no location, as GL's linker
        // drops an inactive varying: a vertex output stays a global of the vertex stage (a
        // struct's already does), so a pack writing more than it reads keeps within the GPU's
        // varyings.
        // Ours (`hz_FogFragCoord` and the other built-ins, `horizonPatchPos`) are read by code
        // of ours, not the pack's: they always travel.
        let ours = var.name.starts_with("hz_") || var.name.starts_with("horizon");
        if !ours && !reads(f, &var.name) {
            if std::ptr::eq(stage, v) && !var.structure {
                private.push(plain(&var.decl).replace('@', &var.name));
            }
            continue;
        }
        if var.structure {
            match struct_parts(&var.decl, &stage.structs) {
                Some(parts) => {
                    let mut copies = Vec::new();
                    for (k, (decl, members)) in parts.into_iter().enumerate() {
                        let part = format!("hz_v_{}_{k}", var.name);
                        let next = locations.len() as u32;
                        locations.insert(part.clone(), (next, decl));
                        for (swizzle, access) in members {
                            copies.push((format!("{part}{swizzle}"), access));
                        }
                    }
                    split.insert(var.name.clone(), (String::new(), copies));
                }
                None => errors.push(Error {
                    file: stage.pp.files.first().cloned().unwrap_or_default(),
                    line: 0,
                    message: format!("`{}`: this struct can't be a varying", var.name),
                }),
            }
            continue;
        }
        match varying_parts(&var.decl) {
            Some((global, parts)) => {
                let mut copies = Vec::new();
                for (k, (decl, access)) in parts.into_iter().enumerate() {
                    let part = format!("hz_v_{}_{k}", var.name);
                    let next = locations.len() as u32;
                    locations.insert(part.clone(), (next, decl));
                    copies.push((part, access));
                }
                split.insert(var.name.clone(), (global, copies));
            }
            None => {
                let next = locations.len() as u32;
                locations.insert(var.name.clone(), (next, var.decl.clone()));
            }
        }
    }
    Varyings {
        locations,
        split,
        private,
    }
}

/// Whether `stage`'s code names `name` outside the declarations moved into the header.
fn reads(stage: &Stage, name: &str) -> bool {
    (stage.tokens.iter().enumerate())
        .any(|(i, t)| *t == name && !stage.removed.iter().any(|r| r.contains(&i)))
}

/// `decl` without its interpolation qualifiers: a plain global's declaration.
fn plain(decl: &str) -> String {
    decl.split(' ')
        .filter(|w| {
            !matches!(
                *w,
                "flat" | "smooth" | "noperspective" | "centroid" | "sample"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A matrix or array varying (`flat mat3 @[2];`): wgpu's stage interfaces take only scalars
/// and vectors (WGSL), so it becomes a private global (`mat3 @[2];`) and one varying per
/// column or element, each with the qualifiers and how to reach it (`[1][2]`). `None` for
/// scalars and vectors, and for sizes that are not plain numbers.
pub(super) fn varying_parts(decl: &str) -> Option<(String, Vec<(String, String)>)> {
    let (before, after) = decl.split_once('@')?;
    let mut words: Vec<&str> = before.split_whitespace().collect();
    let ty = words.pop()?;
    let qualifiers: String = words.iter().map(|w| format!("{w} ")).collect();
    let array = after.trim().trim_end_matches(';').trim();
    let mut sizes = Vec::new();
    let mut rest = array;
    while let Some(r) = rest.strip_prefix('[') {
        let (n, r) = r.split_once(']')?;
        sizes.push(n.trim().parse::<usize>().ok()?);
        rest = r.trim_start();
    }
    if !rest.is_empty() {
        return None;
    }
    // matC or matCxR: C columns of R rows.
    let column = ty.strip_prefix("mat").and_then(|d| {
        let (c, r) = d.split_once('x').unwrap_or((d, d));
        Some((c.parse::<usize>().ok()?, r.parse::<usize>().ok()?))
    });
    if sizes.is_empty() && column.is_none() {
        return None;
    }
    let part_ty = column.map_or(ty.to_owned(), |(_, r)| format!("vec{r}"));
    if let Some((c, _)) = column {
        sizes.push(c);
    }
    let count: usize = sizes.iter().product();
    let parts = (0..count)
        .map(|mut k| {
            let mut access = String::new();
            for &size in sizes.iter().rev() {
                access.insert_str(0, &format!("[{}]", k % size));
                k /= size;
            }
            (format!("{qualifiers}{part_ty} @;"), access)
        })
        .collect();
    Some((format!("{ty} @{array};"), parts))
}

/// A struct varying (`flat Params @;`) as vectors: its scalar, vector and matrix members, through
/// nested structs and arrays, packed four components at a time by base type. Each part: its
/// declaration and, per member, the swizzle of the part and how to reach the member (`.a[1].b`).
/// `None` for a member that can't travel (`bool`, an unknown type, a size that is not a number).
#[allow(clippy::type_complexity)]
pub(super) fn struct_parts(
    decl: &str,
    structs: &std::collections::HashMap<String, Vec<(String, String)>>,
) -> Option<Vec<(String, Vec<(String, String)>)>> {
    let (before, after) = decl.split_once('@')?;
    let mut words: Vec<&str> = before.split_whitespace().collect();
    let ty = words.pop()?;
    let qualifiers: String = words.iter().map(|w| format!("{w} ")).collect();
    let mut leaves = Vec::new();
    leaves_of(
        ty,
        after.trim().trim_end_matches(';'),
        String::new(),
        structs,
        &mut leaves,
    )?;
    // Slots of one base type and up to four components: (base, used, members (from, size, access)).
    let mut slots: Vec<(&str, usize, Vec<(usize, usize, String)>)> = Vec::new();
    for (base, size, access) in leaves {
        match slots
            .iter_mut()
            .find(|(b, used, _)| *b == base && used + size <= 4)
        {
            Some(slot) => {
                slot.2.push((slot.1, size, access));
                slot.1 += size;
            }
            None => slots.push((base, size, vec![(0, size, access)])),
        }
    }
    Some(
        slots
            .into_iter()
            .map(|(base, used, members)| {
                let ty = match (base, used) {
                    (b, 1) => b.to_owned(),
                    ("float", n) => format!("vec{n}"),
                    ("int", n) => format!("ivec{n}"),
                    (_, n) => format!("uvec{n}"),
                };
                // Integers only travel flat.
                let flat = if base != "float" && !qualifiers.contains("flat") {
                    "flat "
                } else {
                    ""
                };
                let members = members
                    .into_iter()
                    .map(|(from, size, access)| {
                        let swizzle = if used == 1 {
                            String::new()
                        } else {
                            format!(".{}", &"xyzw"[from..from + size])
                        };
                        (swizzle, access)
                    })
                    .collect();
                (format!("{flat}{qualifiers}{ty} @;"), members)
            })
            .collect(),
    )
}

/// The scalars and vectors inside a value of type `ty` with array suffix `array`, reached by
/// `access`: (base type, components, access).
fn leaves_of(
    ty: &str,
    array: &str,
    access: String,
    structs: &std::collections::HashMap<String, Vec<(String, String)>>,
    out: &mut Vec<(&'static str, usize, String)>,
) -> Option<()> {
    if let Some(rest) = array.trim().strip_prefix('[') {
        let (n, rest) = rest.split_once(']')?;
        for k in 0..n.trim().parse::<usize>().ok()? {
            leaves_of(ty, rest, format!("{access}[{k}]"), structs, out)?;
        }
        return Some(());
    }
    if let Some(members) = structs.get(ty) {
        for (member_ty, member) in members {
            let (name, array) = member
                .split_once('[')
                .map_or((member.as_str(), ""), |(n, _)| (n, &member[n.len()..]));
            leaves_of(member_ty, array, format!("{access}.{name}"), structs, out)?;
        }
        return Some(());
    }
    if let Some(dims) = ty.strip_prefix("mat") {
        let (c, r) = dims.split_once('x').unwrap_or((dims, dims));
        let (c, r) = (c.parse::<usize>().ok()?, r.parse::<usize>().ok()?);
        for k in 0..c {
            out.push(("float", r, format!("{access}[{k}]")));
        }
        return Some(());
    }
    let (base, size) = match ty {
        "float" => ("float", 1),
        "int" => ("int", 1),
        "uint" => ("uint", 1),
        _ => {
            let (base, n) = if let Some(n) = ty.strip_prefix("vec") {
                ("float", n)
            } else if let Some(n) = ty.strip_prefix("ivec") {
                ("int", n)
            } else {
                ("uint", ty.strip_prefix("uvec")?)
            };
            (base, n.parse().ok()?)
        }
    };
    out.push((base, size, access));
    Some(())
}
