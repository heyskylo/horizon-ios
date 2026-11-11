//! GLSL that vanilla's GL compiler takes and naga's front end doesn't (#9386), rewritten into
//! what both take: a function with `sampler2D` parameters is copied once per set of samplers its
//! callers pass (all of them a shader's global samplers), the copy reading those samplers by name.

/// The longest shader rewritten (vanilla's, includes expanded, are a few KiB).
const MAX_LEN: usize = 64 * 1024;

/// The bytes the rewrite may scan for calls (the whole text per call it rewrites): thousands of
/// calls leave the text as it was rather than freeze a load.
const SCAN_BUDGET: usize = 64 << 20;

/// `text` with the rewrites, where they apply; anything they can't follow is left as it was.
pub(super) fn fixups(text: &str) -> String {
    if text.len() > MAX_LEN {
        return text.to_owned();
    }
    specialize_sampler_params(text)
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A name a sampler can have (`sampler2D ;` gives none, `t[2]` isn't one).
fn is_name(s: &str) -> bool {
    s.chars().all(is_ident) && s.starts_with(|c: char| !c.is_ascii_digit()) && s != "sampler2D"
}

/// The identifier that ends right before byte `end` (whitespace skipped), and where it starts.
fn ident_before(text: &str, end: usize) -> Option<(usize, &str)> {
    let trimmed = text[..end].trim_end();
    let start = trimmed
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()?
        .0;
    let ident = &trimmed[start..];
    (!ident.is_empty() && !ident.starts_with(|c: char| c.is_ascii_digit()))
        .then_some((start, ident))
}

/// For each byte of `text`, the byte of the bracket closing the one opening there (counting
/// that kind of bracket only), or `None`: one pass, so a scan asking for many stays linear.
fn closings(text: &str) -> Vec<Option<usize>> {
    let mut out = vec![None; text.len()];
    let mut open: [Vec<usize>; 3] = Default::default();
    for (i, b) in text.bytes().enumerate() {
        let (kind, opens) = match b {
            b'(' => (0, true),
            b')' => (0, false),
            b'[' => (1, true),
            b']' => (1, false),
            b'{' => (2, true),
            b'}' => (2, false),
            _ => continue,
        };
        if opens {
            open[kind].push(i);
        } else if let Some(at) = open[kind].pop() {
            out[at] = Some(i);
        }
    }
    out
}

/// The comma-separated parts of `list` at its own nesting level.
fn split_top(list: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in list.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !list[start..].trim().is_empty() || !parts.is_empty() {
        parts.push(&list[start..]);
    }
    parts
}

/// A function at the file's top level whose parameters include a `sampler2D`.
struct SamplerFn {
    name: String,
    /// The header from its return type to its `{` (or `;` for a prototype), and the body after.
    start: usize,
    end: usize,
    params: Vec<String>,
    /// The indices of the `sampler2D` parameters and their names.
    samplers: Vec<(usize, String)>,
    /// `None` for a prototype.
    body: Option<String>,
    return_type: String,
}

fn sampler_functions(text: &str) -> Vec<SamplerFn> {
    let closing = closings(text);
    let mut out = Vec::new();
    let mut depth = 0i32;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'(' if depth == 0 => {
                if let (Some((name_at, name)), Some(close)) = (ident_before(text, i), closing[i])
                    && let Some((type_at, return_type)) = ident_before(text, name_at)
                {
                    let params: Vec<String> = split_top(&text[i + 1..close])
                        .into_iter()
                        .map(|p| p.trim().to_owned())
                        .collect();
                    let samplers: Vec<(usize, String)> = params
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| p.split_whitespace().any(|w| w == "sampler2D"))
                        .map(|(n, p)| (n, p.split_whitespace().last().unwrap_or("").to_owned()))
                        .collect();
                    // One without a name is left for naga to refuse.
                    if !samplers.iter().all(|(_, name)| is_name(name)) {
                        i = close + 1;
                        continue;
                    }
                    let after = text[close + 1..].trim_start();
                    let next = close + 1 + (text[close + 1..].len() - after.len());
                    if !samplers.is_empty() && (after.starts_with('{') || after.starts_with(';')) {
                        let (end, body) = if after.starts_with('{') {
                            let Some(body_end) = closing[next] else {
                                break;
                            };
                            (body_end + 1, Some(text[next..=body_end].to_owned()))
                        } else {
                            (next + 1, None)
                        };
                        out.push(SamplerFn {
                            name: name.to_owned(),
                            start: type_at,
                            end,
                            params,
                            samplers,
                            body,
                            return_type: return_type.to_owned(),
                        });
                        i = end;
                        continue;
                    }
                }
                // Any other header: no other one sits in its parentheses.
                if let Some(close) = closing[i] {
                    i = close + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The calls to `name` outside `skip` (its own header): each call's span and arguments. The
/// bytes read are added to `scanned`.
fn calls<'a>(
    scanned: &std::cell::Cell<usize>,
    text: &'a str,
    name: &str,
    skip: &[(usize, usize)],
) -> Vec<(usize, usize, Vec<&'a str>)> {
    scanned.set(scanned.get() + text.len());
    let closing = closings(text);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before = text[..at].chars().next_back();
        let rest = &text[at + name.len()..];
        let open = at + name.len() + (rest.len() - rest.trim_start().len());
        if before.is_some_and(is_ident)
            || !text[open..].starts_with('(')
            || skip.iter().any(|(s, e)| (*s..*e).contains(&at))
        {
            continue;
        }
        let Some(close) = closing[open] else {
            continue;
        };
        scanned.set(scanned.get() + close - open);
        out.push((at, close + 1, split_top(&text[open + 1..close])));
    }
    out
}

/// Every `from` standing as an identifier on its own, but not a member after a `.` (a
/// parameter `s` leaves the swizzle `c.s` alone).
fn replace_ident(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    while let Some(found) = text[done..].find(from) {
        let at = done + found;
        // Read in the whole text: a match right after another (`ss`) is part of it.
        let before = text[..at].trim_end().chars().next_back();
        let joined = text[..at].chars().next_back().is_some_and(is_ident);
        let after = text[at + from.len()..].chars().next();
        out.push_str(&text[done..at]);
        if joined || before == Some('.') || after.is_some_and(is_ident) {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        done = at + from.len();
    }
    out.push_str(&text[done..]);
    out
}

/// A body with its sampler parameters reading `chosen`, all at once: a parameter named like
/// another's sampler (`mix2(sampler2D Sampler0, sampler2D Sampler1)` called with them swapped)
/// still reads its own.
fn read_samplers(body: &str, params: &[(usize, String)], chosen: &[String]) -> String {
    let mut body = body.to_owned();
    for (n, (_, param)) in params.iter().enumerate() {
        body = replace_ident(&body, param, &format!("hz_sampler_param_{n}"));
    }
    for (n, sampler) in chosen.iter().enumerate().take(params.len()) {
        body = replace_ident(&body, &format!("hz_sampler_param_{n}"), sampler);
    }
    body
}

/// The byte after `uniform sampler2D <name>;`, where the shader declares that sampler.
fn sampler_declaration_end(text: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find("sampler2D") {
        let at = from + found;
        from = at + "sampler2D".len();
        let before = text[..at].trim_end();
        if !before.ends_with("uniform")
            || before[..before.len() - 7]
                .chars()
                .next_back()
                .is_some_and(is_ident)
        {
            continue;
        }
        let rest = &text[from..];
        let ident = rest.trim_start();
        let len = ident.chars().take_while(|c| is_ident(*c)).count();
        if &ident[..len] != name {
            continue;
        }
        let after = ident[len..].trim_start();
        if after.starts_with(';') {
            return Some(text.len() - after.len() + 1);
        }
    }
    None
}

/// The samplers a call passes for `f`'s `sampler2D` parameters, in order.
fn chosen_samplers(f: &SamplerFn, args: &[&str]) -> Vec<String> {
    f.samplers
        .iter()
        .filter_map(|(n, _)| args.get(*n).map(|a| a.trim().to_owned()))
        .collect()
}

/// One pass per function with `sampler2D` parameters, until none is left or one can't be
/// followed (a caller passes something that is no sampler's name).
fn specialize_sampler_params(original: &str) -> String {
    let mut text = original.to_owned();
    // Bytes scanned for calls so far, against [`SCAN_BUDGET`].
    let scanned = std::cell::Cell::new(0usize);
    let spent = || scanned.get() > SCAN_BUDGET;
    for _ in 0..16 {
        let functions = sampler_functions(&text);
        // From the outside in: no call inside another function still to be copied.
        let inside_another = |f: &SamplerFn, at: usize| {
            functions
                .iter()
                .any(|g| g.name != f.name && g.body.is_some() && (g.start..g.end).contains(&at))
        };
        let spans_of = |f: &SamplerFn| -> Vec<(usize, usize)> {
            functions
                .iter()
                .filter(|g| g.name == f.name)
                .map(|g| (g.start, g.end))
                .collect()
        };
        let Some(f) = functions.iter().find(|f| {
            f.body.is_some()
                && !spent()
                && calls(&scanned, &text, &f.name, &spans_of(f))
                    .iter()
                    .all(|(at, ..)| !inside_another(f, *at))
        }) else {
            if spent() {
                break;
            }
            // None left to copy.
            return text;
        };
        let spans = spans_of(f);
        let found = calls(&scanned, &text, &f.name, &spans);
        // Each call's samplers must be plain names (a global's, or a later pass's parameter).
        let mut variants: Vec<Vec<String>> = Vec::new();
        let mut call_starts: Vec<usize> = Vec::new();
        let mut followable = true;
        for (at, _, args) in &found {
            let chosen = chosen_samplers(f, args);
            if chosen.len() != f.samplers.len() || !chosen.iter().all(|a| is_name(a)) {
                followable = false;
                break;
            }
            call_starts.push(*at);
            if !variants.contains(&chosen) {
                variants.push(chosen);
            }
            if variants.len() > 64 {
                followable = false;
                break;
            }
        }
        // Overloads (`tap(sampler2D, vec2)` and `tap(sampler2D, vec3)`) each keep their own
        // body; a call picks its samplers by position, so they must sit at the same places.
        let same_places = functions.iter().filter(|g| g.name == f.name).all(|g| {
            g.samplers
                .iter()
                .map(|s| s.0)
                .eq(f.samplers.iter().map(|s| s.0))
        });
        if !followable || !same_places || spent() {
            break;
        }
        // A body may call another overload of itself with its own sampler (vanilla's
        // `sampleNearest` calls the six-argument one): once its sampler parameters read a
        // sampler, those calls name that copy too, and their sets join the variants.
        let rewrite_own_calls = |body: &str| -> Option<(String, Vec<Vec<String>>)> {
            let mut body = body.to_owned();
            let mut found = Vec::new();
            let mut starts: Vec<usize> = calls(&scanned, &body, &f.name, &[])
                .iter()
                .map(|c| c.0)
                .collect();
            starts.sort_unstable_by(|a, b| b.cmp(a));
            for start in starts {
                if spent() {
                    return None;
                }
                let (end, chosen, with) = {
                    let (_, end, args) = calls(&scanned, &body, &f.name, &[])
                        .into_iter()
                        .find(|c| c.0 == start)?;
                    let chosen = chosen_samplers(f, &args);
                    let rest: Vec<&str> = args
                        .iter()
                        .enumerate()
                        .filter(|(n, _)| !f.samplers.iter().any(|(s, _)| s == n))
                        .map(|(_, a)| a.trim())
                        .collect();
                    let with = format!("{}_hz_{}({})", f.name, chosen.join("_"), rest.join(", "));
                    (end, chosen, with)
                };
                if chosen.len() != f.samplers.len() || !chosen.iter().all(|a| is_name(a)) {
                    return None;
                }
                found.push(chosen);
                body.replace_range(start..end, &with);
            }
            Some((body, found))
        };
        let specialize = |g: &SamplerFn, chosen: &[String]| {
            let body = read_samplers(g.body.as_deref().unwrap_or_default(), &g.samplers, chosen);
            rewrite_own_calls(&body)
        };
        let same: Vec<&SamplerFn> = functions.iter().filter(|g| g.name == f.name).collect();
        let mut next = 0;
        while followable && next < variants.len() {
            let chosen = variants[next].clone();
            for g in same.iter().filter(|g| g.body.is_some()) {
                match specialize(g, &chosen) {
                    Some((_, inner)) => {
                        for v in inner {
                            if !variants.contains(&v) {
                                variants.push(v);
                            }
                        }
                    }
                    None => followable = false,
                }
            }
            next += 1;
            followable &= variants.len() <= 64 && !spent();
        }
        if !followable {
            break;
        }
        // One copy of an overload per set of samplers its callers pass, keeping its other
        // parameters; a prototype becomes the copies' prototypes.
        let signature = |g: &SamplerFn, chosen: &[String]| {
            let params: Vec<&str> = g
                .params
                .iter()
                .enumerate()
                .filter(|(n, _)| !g.samplers.iter().any(|(s, _)| s == n))
                .map(|(_, p)| p.as_str())
                .collect();
            format!(
                "{} {}_hz_{}({})",
                g.return_type,
                g.name,
                chosen.join("_"),
                params.join(", ")
            )
        };
        let copies_of = |g: &SamplerFn| -> String {
            variants
                .iter()
                .map(|chosen| match &g.body {
                    Some(_) => {
                        let body = specialize(g, chosen).map(|(b, _)| b).unwrap_or_default();
                        format!("{} {body}\n", signature(g, chosen))
                    }
                    None => format!("{};\n", signature(g, chosen)),
                })
                .collect()
        };
        // Rebuild from the back: the calls, then the function's own spans (each prototype and
        // definition becomes its copies). A call nested in another's arguments starts after it,
        // so it is rewritten first, and the call around it is read again from the text as it is
        // then. A copy reads its samplers by name, so definitions above their samplers'
        // declaration (vanilla's `sample_lightmap.glsl` is included above `uniform sampler2D
        // Sampler2;`) move after it, together and in their order, as long as no call comes first.
        let first_call = call_starts.iter().copied().min().unwrap_or(usize::MAX);
        let declared = variants
            .iter()
            .flatten()
            .filter_map(|s| sampler_declaration_end(&text, s))
            .max();
        let mut edits: Vec<(usize, Option<(usize, String)>)> =
            call_starts.into_iter().map(|at| (at, None)).collect();
        let (mut moved, mut move_to) = (String::new(), None);
        for g in &same {
            match (
                &g.body,
                declared.filter(|&end| end > g.start && end <= first_call),
            ) {
                (Some(_), Some(end)) => {
                    edits.push((g.start, Some((g.end, String::new()))));
                    moved.push_str(&copies_of(g));
                    move_to = Some(end);
                }
                _ => edits.push((g.start, Some((g.end, copies_of(g))))),
            }
        }
        if let Some(end) = move_to {
            edits.push((end, Some((end, format!("\n{moved}")))));
        }
        // Pack text is untrusted: edits that overlap, or off the text, leave it as it was.
        let mut spans: Vec<(usize, usize)> = edits
            .iter()
            .filter_map(|(s, e)| e.as_ref().map(|(end, _)| (*s, *end)))
            .collect();
        spans.sort_unstable();
        if spans.windows(2).any(|w| w[1].0 < w[0].1) {
            break;
        }
        edits.sort_by_key(|(start, _)| std::cmp::Reverse(*start));
        let mut sound = true;
        for (start, edit) in edits {
            let (end, with) = match edit {
                Some(span) => span,
                None => {
                    let found = calls(&scanned, &text, &f.name, &[])
                        .into_iter()
                        .find(|c| c.0 == start);
                    let Some((_, end, args)) = found else {
                        continue;
                    };
                    let rest: Vec<&str> = args
                        .iter()
                        .enumerate()
                        .filter(|(n, _)| !f.samplers.iter().any(|(s, _)| s == n))
                        .map(|(_, a)| a.trim())
                        .collect();
                    let copy = format!("{}_hz_{}", f.name, chosen_samplers(f, &args).join("_"));
                    (end, format!("{copy}({})", rest.join(", ")))
                }
            };
            if start > end || text.get(start..end).is_none() || spent() {
                sound = false;
                break;
            }
            text.replace_range(start..end, &with);
        }
        if !sound {
            break;
        }
    }
    // One that couldn't be followed: the shader as written, never half rewritten.
    original.to_owned()
}

#[cfg(test)]
mod tests;
