//! Resource packs' core shaders (`shaders/core/*.vsh` and `.fsh`: the GLSL vanilla compiles for its
//! pipelines) as naga modules, so a pack that reshapes text from `core/text` (a HUD its vertex
//! shader moves to the screen's corners, effects in its fragment shader) draws as in vanilla.
//!
//! The pack's GLSL (26.3's `#include` or the older `#moj_import`, with explicit locations or
//! without) is written again as the Vulkan-flavoured GLSL 450 naga reads:
//! - GLSL vanilla's GL compiler takes and naga doesn't is rewritten first ([`glsl_fixups`]: a
//!   helper taking the `sampler2D` it reads, an array sized by its initializer only);
//! - includes are inlined, each file once (as `GlslPreprocessor` imported them), from `include`;
//! - vanilla's uniform blocks get the bindings of [`BLOCKS`], and each of [`SAMPLERS`] is split
//!   into a texture and a sampler object, combined again where the sampler is named;
//! - the vertex inputs become globals the caller's [`Glue`] fills before the pack's `main`
//!   (renamed `hz_main`) runs, and `gl_VertexID` and `gl_VertexIndex` read the glue's expression;
//! - varyings get locations by name across the stages, the fragment output location 0.

use std::collections::HashSet;
use std::fmt::Write;

use naga::ShaderStage;
use naga::front::glsl::{Frontend, Options};
use naga::valid::{Capabilities, ValidationFlags, Validator};

mod glsl_fixups;

/// Vanilla's uniform blocks (`shaders/include/*.glsl`) and their bindings in set 0.
pub const BLOCKS: [(&str, u32); 4] = [
    ("Globals", 0),
    ("Projection", 1),
    ("DynamicTransforms", 2),
    ("Fog", 3),
];

/// Vanilla's samplers: the texture's binding in set 0, the sampler object's the next one.
pub const SAMPLERS: [(&str, u32); 3] = [("Sampler0", 4), ("Sampler1", 6), ("Sampler2", 8)];

/// The first binding of the blocks and samplers vanilla has for other pipelines (OIT's behind
/// `#ifdef OIT`): fine while the preprocessor drops them, an error when a stage uses one.
const OTHER_BINDINGS: u32 = 16;

/// What the caller's pipeline gives the pack's vertex shader.
pub struct Glue<'a> {
    /// The inputs of the pipeline's vanilla vertex format: name and GLSL type.
    pub inputs: &'a [(&'a str, &'a str)],
    /// Declarations before the pack's code: the real attributes, helpers.
    pub prelude: &'a str,
    /// Statements before the pack's `main`: `inputs` filled from the attributes.
    pub setup: &'a str,
    /// Statements after it, on `gl_Position`.
    pub finish: &'a str,
    /// The expression the pack's `gl_VertexID` and `gl_VertexIndex` read.
    pub vertex_id: &'a str,
}

/// A pipeline's two stages, validated.
#[derive(Debug)]
pub struct CoreProgram {
    pub vertex: naga::Module,
    pub fragment: naga::Module,
    /// The blocks and samplers that are none of vanilla's, in binding order: the `i`th is at
    /// [`other_binding`]`(i)` (a sampler's object one after its texture; a block leaves that
    /// binding unused).
    pub other: Vec<String>,
}

/// The binding of the `index`th block or sampler of [`CoreProgram::other`].
pub fn other_binding(index: usize) -> u32 {
    OTHER_BINDINGS + 2 * index as u32
}

/// The shaders of one pipeline (`text.vsh`, `text.fsh`) with vanilla's `defines` for it.
/// `include` answers a file id (`minecraft:shaders/include/fog.glsl`) with its text.
pub fn translate_core(
    vertex: &str,
    fragment: &str,
    defines: &[&str],
    include: &dyn Fn(&str) -> Option<String>,
    glue: &Glue,
) -> Result<CoreProgram, String> {
    translate_core_with(vertex, fragment, defines, include, glue, &|_| false)
}

/// [`translate_core`] for a pipeline that also gives the blocks and samplers `provided` says
/// yes to, at their [`CoreProgram::other`] bindings; the others are still an error. `provided`
/// answers a block's name and a sampler's alike: the caller knows which kind each one is.
pub fn translate_core_with(
    vertex: &str,
    fragment: &str,
    defines: &[&str],
    include: &dyn Fn(&str) -> Option<String>,
    glue: &Glue,
    provided: &dyn Fn(&str) -> bool,
) -> Result<CoreProgram, String> {
    const CORE: &str = "minecraft:shaders/core/";
    let vertex_text = glsl_fixups::fixups(&strip_comments(&expand(vertex, CORE, include)?));
    let fragment_text = glsl_fixups::fixups(&strip_comments(&expand(fragment, CORE, include)?));
    let mut names = Names::default();
    let vertex_code = rewrite(&vertex_text, ShaderStage::Vertex, glue, &mut names)
        .map_err(|e| format!("vertex shader: {e}"))?;
    let fragment_code = rewrite(&fragment_text, ShaderStage::Fragment, glue, &mut names)
        .map_err(|e| format!("fragment shader: {e}"))?;
    let vertex = compile(ShaderStage::Vertex, &vertex_code, defines, &names, provided)
        .map_err(|e| format!("vertex shader: {e}"))?;
    let fragment = compile(
        ShaderStage::Fragment,
        &fragment_code,
        defines,
        &names,
        provided,
    )
    .map_err(|e| format!("fragment shader: {e}"))?;
    let given = locations(&vertex, false);
    if let Some(missing) = locations(&fragment, true)
        .into_iter()
        .find(|l| !given.contains(l))
    {
        return Err(format!(
            "the fragment shader reads location {missing}, which the vertex shader doesn't write"
        ));
    }
    Ok(CoreProgram {
        vertex,
        fragment,
        other: names.other,
    })
}

/// A bound on a pack's includes, which are pasted every time.
const MAX_PASTES: u32 = 1024;
const MAX_EXPANDED: usize = 1 << 20;

/// The file with its includes inlined (`#include <ns:path>`, `#include "relative"` and the same
/// with `#moj_import`), `#version` and `#extension` lines dropped. An `#include` is pasted every
/// time, as vanilla's shaderc does (the files' `#ifndef` guards keep one copy); a `#moj_import`
/// once.
fn expand(
    text: &str,
    dir: &str,
    include: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    fn walk(
        text: &str,
        dir: &str,
        include: &dyn Fn(&str) -> Option<String>,
        seen: &mut HashSet<String>,
        pastes: &mut u32,
        depth: u32,
        out: &mut String,
    ) -> Result<(), String> {
        if depth > 16 {
            return Err("includes nest more than 16 deep".into());
        }
        for line in text.lines() {
            let directive = line.trim_start().strip_prefix('#').map(str::trim_start);
            let target = directive.and_then(|d| {
                d.strip_prefix("include")
                    .map(|t| (t.trim(), false))
                    .or_else(|| d.strip_prefix("moj_import").map(|t| (t.trim(), true)))
            });
            if let Some((target, once)) = target {
                let id = include_id(target, dir).ok_or_else(|| format!("bad include {target}"))?;
                if seen.insert(id.clone()) || !once {
                    *pastes += 1;
                    if *pastes > MAX_PASTES {
                        return Err(format!("includes paste more than {MAX_PASTES} files"));
                    }
                    let file = include(&id).ok_or_else(|| format!("{id} not found"))?;
                    let parent = &id[..id.rfind('/').map_or(id.len(), |i| i + 1)];
                    walk(&file, parent, include, seen, pastes, depth + 1, out)?;
                }
                out.push('\n');
            } else if directive
                .is_some_and(|d| d.starts_with("version") || d.starts_with("extension"))
            {
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
            if out.len() > MAX_EXPANDED {
                return Err(format!(
                    "includes grow the shader past {MAX_EXPANDED} bytes"
                ));
            }
        }
        Ok(())
    }
    let mut out = String::new();
    walk(text, dir, include, &mut HashSet::new(), &mut 0, 0, &mut out)?;
    Ok(out)
}

/// `<ns:path>` under `shaders/include` (`minecraft` when there's no namespace), or `"path"` from
/// the including file's folder.
fn include_id(target: &str, dir: &str) -> Option<String> {
    if let Some(inner) = target.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
        let (ns, path) = inner.split_once(':').unwrap_or(("minecraft", inner));
        return Some(format!("{ns}:shaders/include/{path}"));
    }
    let relative = target.strip_prefix('"')?.strip_suffix('"')?;
    let (ns, base) = dir.split_once(':')?;
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(format!("{ns}:{}", parts.join("/")))
}

/// Comments as spaces, line breaks kept.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                while chars.peek().is_some_and(|&c| c != '\n') {
                    chars.next();
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                    }
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `text` with each whole identifier `from` written as `to`.
fn replace_ident(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + from.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_some_and(is_ident) || after.is_some_and(is_ident) {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// A global declaration on one line: `layout(...) qualifiers storage type name;`.
struct Decl<'a> {
    /// `layout(location = N)`'s N.
    location: Option<u32>,
    qualifiers: Vec<&'a str>,
    storage: &'a str,
    ty: &'a str,
    name: &'a str,
}

fn declaration(line: &str) -> Option<Decl<'_>> {
    let mut rest = line.trim();
    let mut location = None;
    if let Some(after) = rest.strip_prefix("layout") {
        let after = after.trim_start().strip_prefix('(')?;
        let close = after.find(')')?;
        location = after[..close].split(',').find_map(|q| {
            let (key, value) = q.split_once('=')?;
            (key.trim() == "location").then(|| value.trim().parse().ok())?
        });
        rest = after[close + 1..].trim_start();
    }
    let body = rest.strip_suffix(';')?.trim_end();
    let words: Vec<&str> = body.split_whitespace().collect();
    let storage_at = words
        .iter()
        .position(|w| matches!(*w, "in" | "out" | "uniform"))?;
    let [ty, name] = words[storage_at + 1..] else {
        return None;
    };
    if !name.chars().all(is_ident) || !ty.chars().all(is_ident) {
        return None;
    }
    Some(Decl {
        location,
        qualifiers: words[..storage_at].to_vec(),
        storage: words[storage_at],
        ty,
        name,
    })
}

/// GLSL pack shaders are often minified, with several global declarations on one line.
/// Keep the line-oriented rewriter simple by separating only declaration statements; ordinary
/// statements and function bodies remain untouched.
fn declaration_lines(line: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    while let Some(relative) = line[start..].find(';') {
        let end = start + relative + 1;
        if declaration(&line[start..end]).is_some() {
            lines.push(&line[start..end]);
            start = end;
        } else {
            break;
        }
    }
    if start == 0 {
        vec![line]
    } else {
        if start < line.len() {
            lines.push(&line[start..]);
        }
        lines
    }
}

/// The block a `uniform Name {` line opens, with the line's text before `uniform`.
fn block_name(line: &str) -> Option<&str> {
    let at = line.find("uniform")?;
    let head = line[..at].trim();
    if !(head.is_empty() || head.starts_with("layout")) {
        return None;
    }
    let rest = line[at + "uniform".len()..].trim();
    let name_end = rest.find(|c: char| !is_ident(c)).unwrap_or(rest.len());
    let (name, tail) = rest.split_at(name_end);
    // `{` alone, or the whole block on the line (`uniform Day { float daylight; };`).
    (!name.is_empty() && (tail.trim().is_empty() || tail.trim_start().starts_with('{')))
        .then_some(name)
}

/// What the two stages share: the vertex stage's outputs and their locations, and the names of
/// the blocks and samplers of other pipelines in binding order.
#[derive(Default)]
struct Names {
    varyings: Vec<(String, u32)>,
    other: Vec<String>,
}

impl Names {
    fn location(&self, varying: &str) -> Option<u32> {
        self.varyings
            .iter()
            .find(|(v, _)| v == varying)
            .map(|(_, location)| *location)
    }

    /// A block or sampler of another pipeline: its binding (two for a sampler).
    fn other_binding(&mut self, name: &str) -> u32 {
        let index = match self.other.iter().position(|n| n == name) {
            Some(index) => index,
            None => {
                self.other.push(name.to_owned());
                self.other.len() - 1
            }
        };
        other_binding(index)
    }
}

/// One stage written again for naga (see the module's notes).
fn rewrite(
    text: &str,
    stage: ShaderStage,
    glue: &Glue,
    names: &mut Names,
) -> Result<String, String> {
    let vertex = stage == ShaderStage::Vertex;
    let mut out = String::from("#version 450 core\n");
    if vertex {
        out.push_str(glue.prelude);
        out.push('\n');
        for (name, ty) in glue.inputs {
            writeln!(out, "{ty} {name};").unwrap();
        }
    }
    // Where the pack's own code starts: the glue above it is left as it is.
    let pack_start = out.len();
    let mut outputs: Vec<&str> = Vec::new();
    let explicit: Vec<u32> = if vertex {
        text.lines()
            .flat_map(declaration_lines)
            .filter_map(declaration)
            .filter(|d| d.storage == "out")
            .filter_map(|d| d.location)
            .collect()
    } else {
        Vec::new()
    };
    for source_line in text.lines() {
        for line in declaration_lines(source_line) {
            if let Some(name) = block_name(line) {
                let binding = BLOCKS
                    .iter()
                    .find(|(block, _)| *block == name)
                    .map_or_else(|| names.other_binding(name), |(_, b)| *b);
                let tail = line[line.find("uniform").unwrap_or(0)..].trim();
                writeln!(out, "layout(std140, set = 0, binding = {binding}) {tail}").unwrap();
                continue;
            }
            let Some(decl) = declaration(line) else {
                out.push_str(line);
                out.push('\n');
                continue;
            };
            let qualifiers = decl.qualifiers.join(" ");
            match decl.storage {
                "uniform" if decl.ty == "sampler2D" => {
                    let binding = SAMPLERS
                        .iter()
                        .find(|(sampler, _)| *sampler == decl.name)
                        .map_or_else(|| names.other_binding(decl.name), |(_, b)| *b);
                    let name = decl.name;
                    writeln!(
                        out,
                        "layout(set = 0, binding = {binding}) uniform texture2D hz_tex_{name};\n\
                     layout(set = 0, binding = {}) uniform sampler hz_smp_{name};\n\
                     #define {name} sampler2D(hz_tex_{name}, hz_smp_{name})",
                        binding + 1
                    )
                    .unwrap();
                }
                "uniform" => {
                    // Some packs keep the legacy standalone declarations in an `#else` branch
                    // alongside 26.x's uniform-block imports. The GLSL compiler selects the
                    // branch later, but the declaration pass sees both; the block already provides
                    // these names, so the legacy declarations must not be emitted or rejected.
                    if matches!(
                        decl.name,
                        "CameraBlockPos"
                            | "GlintAlpha"
                            | "CameraOffset"
                            | "GameTime"
                            | "ScreenSize"
                            | "MenuBlurRadius"
                            | "UseRgss"
                            | "ProjMat"
                            | "TextureMat"
                            | "ModelViewMat"
                            | "ColorModulator"
                            | "FogColor"
                            | "FogEnvironmentalStart"
                            | "FogRenderDistanceStart"
                            | "FogRenderDistanceEnd"
                            | "FogEnd"
                            | "FogStart"
                            | "FogShape"
                    ) {
                        out.push('\n');
                        continue;
                    }
                    return Err(format!(
                        "uniform {} outside a block (26.x's shaders keep them in blocks)",
                        decl.name
                    ));
                }
                "in" if vertex => {
                    if glue.inputs.iter().any(|(name, _)| *name == decl.name) {
                        // Declared by the glue as a global.
                        out.push('\n');
                    } else {
                        // Another format's input (`UV2` behind `#if !defined(IS_GUI)`): the
                        // preprocessor drops it when it is for another pipeline, else it reads 0.
                        writeln!(out, "{} {};", decl.ty, decl.name).unwrap();
                    }
                }
                "out" if vertex => {
                    // One location each: naga takes scalars and vectors only.
                    let location = decl
                        .location
                        .or_else(|| names.location(decl.name))
                        .unwrap_or_else(|| {
                            (0..)
                                .find(|l| {
                                    !explicit.contains(l)
                                        && names.varyings.iter().all(|(_, v)| v != l)
                                })
                                .unwrap_or_default()
                        });
                    let varying = (decl.name.to_owned(), location);
                    if !names.varyings.contains(&varying) {
                        names.varyings.push(varying);
                    }
                    writeln!(
                        out,
                        "layout(location = {location}) {qualifiers} out {} {};",
                        decl.ty, decl.name
                    )
                    .unwrap();
                }
                "in" => {
                    let location = decl
                        .location
                        .or_else(|| names.location(decl.name))
                        .ok_or_else(|| {
                            format!("input {} isn't written by the vertex shader", decl.name)
                        })?;
                    writeln!(
                        out,
                        "layout(location = {location}) {qualifiers} in {} {};",
                        decl.ty, decl.name
                    )
                    .unwrap();
                }
                _ => {
                    // Pipelines with more targets (OIT's, behind `#ifdef OIT`) name them; the
                    // colour this pipeline draws is the one at location 0.
                    let location = decl.location.unwrap_or_else(|| {
                        let at = outputs.iter().position(|o| *o == decl.name);
                        at.unwrap_or_else(|| {
                            outputs.push(decl.name);
                            outputs.len() - 1
                        }) as u32
                    });
                    writeln!(
                        out,
                        "layout(location = {location}) out {} {};",
                        decl.ty, decl.name
                    )
                    .unwrap();
                }
            }
        }
    }
    if !vertex {
        return Ok(out);
    }
    // The pack's `gl_VertexID` and 26.3's `gl_VertexIndex` both mean vanilla's vertex number
    // (four per quad, an `int`): both read the glue's expression, which for a pipeline that
    // draws six vertices per quad turns its own `gl_VertexIndex` back into that number. Only the
    // pack's code is rewritten, so the glue's expression keeps reading the real index.
    let vertex_id = format!("({})", glue.vertex_id);
    let pack = replace_ident(
        &rename_main(&out[pack_start..]),
        "gl_VertexIndex",
        &vertex_id,
    );
    let pack = replace_ident(&pack, "gl_VertexID", &vertex_id);
    let mut code = format!("{}{pack}", &out[..pack_start]);
    write!(
        code,
        "void main() {{\n{}\nhz_main();\n{}\n}}\n",
        glue.setup, glue.finish
    )
    .unwrap();
    Ok(code)
}

/// `void main()` becomes `void hz_main()`.
fn rename_main(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("main") {
        let before = rest[..at].trim_end();
        let after = rest[at + 4..].trim_start();
        let whole = !rest[..at].chars().next_back().is_some_and(is_ident)
            && !rest[at + 4..].chars().next().is_some_and(is_ident);
        out.push_str(&rest[..at]);
        if whole && after.starts_with('(') && before.ends_with("void") {
            out.push_str("hz_main");
        } else {
            out.push_str("main");
        }
        rest = &rest[at + 4..];
    }
    out.push_str(rest);
    out
}

/// Parses and validates one stage; errors name the line of the rewritten code.
fn compile(
    stage: ShaderStage,
    code: &str,
    defines: &[&str],
    names: &Names,
    provided: &dyn Fn(&str) -> bool,
) -> Result<naga::Module, String> {
    let mut options = Options::from(stage);
    for define in defines {
        let (name, value) = define.split_once(' ').unwrap_or((define, ""));
        options.defines.insert(name.to_owned(), value.to_owned());
    }
    let module = Frontend::default().parse(&options, code).map_err(|e| {
        e.errors
            .iter()
            .map(|e| {
                let start = e.meta.to_range().map_or(0, |r| r.start);
                let line = code[..start.min(code.len())].matches('\n').count() + 1;
                let source = code.lines().nth(line - 1).unwrap_or("").trim();
                format!("line {line} ({source}): {}", e.kind)
            })
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| {
            let inner = e.into_inner();
            let mut message = inner.to_string();
            let mut source = std::error::Error::source(&inner);
            while let Some(s) = source {
                write!(message, ": {s}").unwrap();
                source = s.source();
            }
            message
        })?;
    let entry = info.get_entry_point(0);
    for (handle, global) in module.global_variables.iter() {
        let Some(binding) = global.binding.as_ref().map(|b| b.binding) else {
            continue;
        };
        if binding >= OTHER_BINDINGS && !entry[handle].is_empty() {
            // The name it was declared with (a block's instance may have none).
            let index = ((binding - OTHER_BINDINGS) / 2) as usize;
            let name = names.other.get(index).map_or("?", String::as_str);
            if provided(name) {
                continue;
            }
            return Err(format!("{name} is not a block or sampler of this pipeline"));
        }
    }
    Ok(module)
}

/// The locations the entry point reads (`inputs`) or writes.
fn locations(module: &naga::Module, inputs: bool) -> Vec<u32> {
    let mut out = Vec::new();
    let mut add = |ty: naga::Handle<naga::Type>, binding: Option<&naga::Binding>| {
        if let Some(naga::Binding::Location { location, .. }) = binding {
            out.push(*location);
        } else if let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner {
            for m in members {
                if let Some(naga::Binding::Location { location, .. }) = &m.binding {
                    out.push(*location);
                }
            }
        }
    };
    for entry in &module.entry_points {
        if inputs {
            for arg in &entry.function.arguments {
                add(arg.ty, arg.binding.as_ref());
            }
        } else if let Some(result) = &entry.function.result {
            add(result.ty, result.binding.as_ref());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INCLUDES: [(&str, &str); 4] = [
        (
            "minecraft:shaders/include/dynamictransforms.glsl",
            "#ifndef DT\n#define DT\nlayout(std140) uniform DynamicTransforms {\n    mat4 ModelViewMat;\n    mat4 TextureMat;\n    vec4 ColorModulator;\n    vec3 ModelOffset;\n};\n#endif\n",
        ),
        (
            "minecraft:shaders/include/projection.glsl",
            "layout(std140) uniform Projection {\n    mat4 ProjMat;\n};\n",
        ),
        (
            "minecraft:shaders/include/globals.glsl",
            "layout(std140) uniform Globals {\n    ivec3 CameraBlockPos;\n    float GlintAlpha;\n    vec3 CameraOffset;\n    float GameTime;\n    vec2 ScreenSize;\n    int MenuBlurRadius;\n    int UseRgss;\n};\n",
        ),
        (
            "minecraft:shaders/include/fog.glsl",
            "layout(std140) uniform Fog {\n    vec4 FogColor;\n    float FogEnvironmentalStart;\n};\nvec4 apply_fog(vec4 c) { return mix(c, FogColor, FogEnvironmentalStart); }\n",
        ),
    ];

    fn include(id: &str) -> Option<String> {
        INCLUDES
            .iter()
            .find(|(path, _)| *path == id)
            .map(|(_, text)| (*text).to_owned())
    }

    const GLUE: Glue = Glue {
        inputs: &[("Position", "vec3"), ("Color", "vec4"), ("UV0", "vec2")],
        prelude: "layout(location = 0) in vec2 hz_pos;\nlayout(location = 1) in vec2 hz_uv;\nlayout(location = 2) in vec4 hz_color;\n\
                  const int hz_corner[6] = int[6](0, 3, 2, 0, 2, 1);\n\
                  int hz_vertex_id() { int i = int(gl_VertexIndex) - int(gl_InstanceIndex); return i / 6 * 4 + hz_corner[i % 6]; }",
        setup: "Position = vec3(hz_pos, 0.0); Color = hz_color; UV0 = hz_uv;",
        finish: "gl_Position.z = 0.5 * gl_Position.w;",
        vertex_id: "hz_vertex_id()",
    };

    /// 26.3's `text.vsh` and `text.fsh` in shape: `#include`, explicit locations, an extension.
    const VANILLA_VSH: &str = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
        #if !defined(IS_GUI)\n#include <minecraft:fog.glsl>\n#endif\n\
        #include <minecraft:dynamictransforms.glsl>\n#include <minecraft:projection.glsl>\n\
        layout(location = 0) in vec3 Position;\nlayout(location = 1) in vec4 Color;\nlayout(location = 2) in vec2 UV0;\n\
        layout(location = 2) out vec4 vertexColor;\nlayout(location = 3) out vec2 texCoord0;\n\
        void main() {\n    gl_Position = ProjMat * ModelViewMat * vec4(Position, 1.0);\n    vertexColor = Color;\n    texCoord0 = UV0;\n}\n";
    const VANILLA_FSH: &str = "#version 330\n#extension GL_ARB_separate_shader_objects : require\n\
        #include <minecraft:dynamictransforms.glsl>\n\
        uniform sampler2D Sampler0;\n\
        layout(location = 2) in vec4 vertexColor;\nlayout(location = 3) in vec2 texCoord0;\n\
        layout(location = 0) out vec4 fragColor;\n\
        void main() {\n    vec4 color = texture(Sampler0, texCoord0) * vertexColor * ColorModulator;\n    if (color.a < 0.1) {\n        discard;\n    }\n    fragColor = color;\n}\n";

    #[test]
    fn vanilla_26_3_text_shaders_translate() {
        let program =
            translate_core(VANILLA_VSH, VANILLA_FSH, &["IS_GUI"], &include, &GLUE).unwrap();
        assert_eq!(program.vertex.entry_points.len(), 1);
        assert_eq!(program.fragment.entry_points.len(), 1);
    }

    /// A 26.3 pack's `gl_VertexIndex` means vanilla's vertex number, four per quad. The text
    /// pipeline draws six per glyph, so the pack's code must read the glue's `hz_vertex_id()`
    /// (as `gl_VertexID` does), never the raw index.
    #[test]
    fn a_packs_gl_vertex_index_reads_the_glues_vertex_number() {
        let vsh = "#version 330\n#include <minecraft:dynamictransforms.glsl>\n#include <minecraft:projection.glsl>\n\
            in vec3 Position;\nin vec4 Color;\nin vec2 UV0;\nout vec4 vertexColor;\nout vec2 texCoord0;\n\
            void main() {\n    int corner = gl_VertexIndex % 4;\n\
                gl_Position = ProjMat * ModelViewMat * vec4(Position + vec3(float(corner), 0.0, 0.0), 1.0);\n\
                vertexColor = Color;\n    texCoord0 = UV0;\n}\n";
        let program = translate_core(vsh, VANILLA_FSH, &["IS_GUI"], &include, &GLUE).unwrap();
        let module = &program.vertex;
        let named = |name: &str| {
            module
                .functions
                .iter()
                .find(|(_, f)| f.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("no {name}"))
        };
        let (vertex_id, _) = named("hz_vertex_id");
        let (_, pack_main) = named("hz_main");
        fn calls(block: &naga::Block, target: naga::Handle<naga::Function>) -> bool {
            block.iter().any(|statement| match statement {
                naga::Statement::Call { function, .. } => *function == target,
                naga::Statement::Block(inner) => calls(inner, target),
                naga::Statement::If { accept, reject, .. } => {
                    calls(accept, target) || calls(reject, target)
                }
                naga::Statement::Loop {
                    body, continuing, ..
                } => calls(body, target) || calls(continuing, target),
                _ => false,
            })
        }
        assert!(calls(&pack_main.body, vertex_id));
    }

    /// A 26.2-style pack shader as a HUD pack writes it: `#moj_import`, no locations, a flat
    /// varying, `gl_VertexID`, `texelFetch` and `textureSize` on the glyph atlas, `GameTime`.
    #[test]
    fn a_hud_pack_text_shader_translates() {
        let vsh = "#version 330\n\
            #moj_import <minecraft:dynamictransforms.glsl>\n#moj_import <minecraft:projection.glsl>\n\
            in vec3 Position;\nin vec4 Color;\nin vec2 UV0;\n\
            out vec4 vertexColor;\nout vec2 texCoord0;\n\
            #ifdef IS_GUI\n#moj_import <minecraft:globals.glsl>\nuniform sampler2D Sampler0; // the glyphs\nflat out vec4 effect;\n#endif\n\
            /* moved\n   glyphs */\n\
            void main() {\n    vec3 p = Position;\n#ifdef IS_GUI\n    int corner = gl_VertexID % 4;\n\
                ivec2 size = textureSize(Sampler0, 0);\n\
                if (ivec4(round(texelFetch(Sampler0, ivec2(UV0 * vec2(size)), 0) * 255.0)) == ivec4(177, 126, 91, 1)) {\n\
                    p.x += float(corner) + ScreenSize.x * GameTime;\n    }\n    effect = vec4(float((ivec3(Color.rgb * 255.0) >> 4) == ivec3(3, 14, 10)));\n#endif\n\
                gl_Position = ProjMat * ModelViewMat * vec4(p, 1.0);\n    vertexColor = Color;\n    texCoord0 = UV0;\n}\n";
        let fsh = "#version 330\n#moj_import <minecraft:dynamictransforms.glsl>\n#moj_import <minecraft:globals.glsl>\n\
            uniform sampler2D Sampler0;\nin vec4 vertexColor;\nin vec2 texCoord0;\nflat in vec4 effect;\nout vec4 fragColor;\n\
            void main() {\n    vec4 color = texture(Sampler0, texCoord0) * vertexColor;\n    if (effect.x > 0.5) {\n        fragColor = vertexColor * ColorModulator;\n        return;\n    }\n\
                if (color.a < 0.1) {\n        discard;\n    }\n    fragColor = color * sin(GameTime);\n}\n";
        translate_core(vsh, fsh, &["IS_GUI"], &include, &GLUE).unwrap();
    }

    #[test]
    fn varyings_meet_at_the_same_locations() {
        let vsh = "#version 330\nin vec3 Position;\nout float b;\nout vec2 m;\nout vec4 a;\n\
            void main() { gl_Position = vec4(Position, 1.0); b = 1.0; m = vec2(1.0); a = vec4(b); }\n";
        let fsh = "#version 330\nin vec4 a;\nin float b;\nout vec4 fragColor;\nvoid main() { fragColor = a * b; }\n";
        let program = translate_core(vsh, fsh, &[], &include, &GLUE).unwrap();
        let mut read = locations(&program.fragment, true);
        read.sort_unstable();
        assert_eq!(read, [0, 2]);
    }

    #[test]
    fn explicit_varying_locations_are_kept() {
        let vsh = "#version 330\nin vec3 Position;\n\
            #ifdef IS_GUI\nlayout(location = 4) out float gui;\n#else\nlayout(location = 4) out float world;\n#endif\n\
            layout(location = 13) out vec4 a;\nout float b;\n\
            void main() { gl_Position = vec4(Position, 1.0); a = vec4(1.0); b = 1.0;\n\
            #ifdef IS_GUI\ngui = 1.0;\n#else\nworld = 1.0;\n#endif\n}\n";
        let fsh = "#version 330\n#ifdef IS_GUI\nlayout(location = 4) in float gui;\n#endif\n\
            layout(location = 13) in vec4 a;\nin float b;\nout vec4 fragColor;\n\
            void main() { fragColor = a * b;\n#ifdef IS_GUI\nfragColor *= gui;\n#endif\n}\n";
        let program = translate_core(vsh, fsh, &["IS_GUI"], &include, &GLUE).unwrap();
        let mut read = locations(&program.fragment, true);
        read.sort_unstable();
        assert_eq!(read, [0, 4, 13]);
    }

    #[test]
    fn an_include_is_pasted_every_time_and_a_moj_import_once() {
        let file = |id: &str| {
            (id == "minecraft:shaders/include/g.glsl")
                .then(|| "#ifndef G\n#define G\nconst float g = 1.0;\n#endif\n".to_owned())
        };
        let twice =
            "#ifdef A\n#include <minecraft:g.glsl>\n#else\n#include <minecraft:g.glsl>\n#endif\n";
        let expanded = expand(twice, "minecraft:shaders/core/", &file).unwrap();
        assert_eq!(expanded.matches("const float g").count(), 2);
        let imported = expand(
            &twice.replace("#include", "#moj_import"),
            "minecraft:shaders/core/",
            &file,
        )
        .unwrap();
        assert_eq!(imported.matches("const float g").count(), 1);
    }

    #[test]
    fn a_location_another_branch_gave_the_same_name_isnt_reused() {
        let vsh = "#version 330\nin vec3 Position;\n\
            #ifdef IS_GUI\nlayout(location = 0) out vec4 vertexColor;\n\
            #else\nlayout(location = 1) out vec4 vertexColor;\nlayout(location = 0) out vec4 worldOther;\n#endif\n\
            void main() { gl_Position = vec4(Position, 1.0); vertexColor = vec4(1.0);\n\
            #ifndef IS_GUI\nworldOther = vec4(0.5);\n#endif\n}\n";
        let fsh = "#version 330\n\
            #ifdef IS_GUI\nlayout(location = 0) in vec4 vertexColor;\n\
            #else\nlayout(location = 1) in vec4 vertexColor;\nlayout(location = 0) in vec4 worldOther;\n#endif\n\
            out vec4 fragColor;\nvoid main() { fragColor = vertexColor;\n\
            #ifndef IS_GUI\nfragColor *= worldOther;\n#endif\n}\n";
        let world = translate_core(vsh, fsh, &[], &include, &GLUE).unwrap();
        let mut written = locations(&world.vertex, false);
        written.sort_unstable();
        assert_eq!(written, [0, 1]);
        let mut read = locations(&world.fragment, true);
        read.sort_unstable();
        assert_eq!(read, [0, 1]);
        let gui = translate_core(vsh, fsh, &["IS_GUI"], &include, &GLUE).unwrap();
        assert_eq!(locations(&gui.vertex, false), [0]);
    }

    #[test]
    fn includes_that_multiply_are_refused_once_over_budget() {
        let file = |id: &str| {
            let level: u32 = id
                .strip_prefix("minecraft:shaders/include/l")?
                .strip_suffix(".glsl")?
                .parse()
                .ok()?;
            let next = format!("#include <minecraft:l{}.glsl>\n", level + 1).repeat(4);
            Some(format!(
                "#ifndef L{level}\n#define L{level}\n{}#endif\n",
                if level < 15 { next.as_str() } else { "" }
            ))
        };
        let error = expand(
            "#include <minecraft:l0.glsl>\n",
            "minecraft:shaders/core/",
            &file,
        )
        .unwrap_err();
        assert!(error.contains("paste more than"), "{error}");
    }

    #[test]
    fn a_define_with_a_value_is_defined_to_it() {
        let vsh = "#version 330\nin vec3 Position;\n\
            void main() { gl_Position = vec4(Position, float(COUNT)); }\n";
        let fsh =
            "#version 330\nout vec4 fragColor;\nvoid main() { fragColor = vec4(COUNT / 4); }\n";
        assert!(translate_core(vsh, fsh, &["COUNT 8"], &include, &GLUE).is_ok());
    }

    #[test]
    fn minified_varying_declarations_are_rewritten() {
        let vsh = "#version 330\nout float sphericalVertexDistance;out float cylindricalVertexDistance;\
            void main() { sphericalVertexDistance = 0.0; cylindricalVertexDistance = 0.0; \
            gl_Position = vec4(0.0); }\n";
        let fsh = "#version 330\nin float sphericalVertexDistance;in float cylindricalVertexDistance;\
            out vec4 fragColor;void main() { fragColor = vec4(sphericalVertexDistance + cylindricalVertexDistance); }\n";
        translate_core(vsh, fsh, &[], &include, &GLUE).unwrap();
    }

    #[test]
    fn legacy_uniforms_next_to_modern_blocks_are_ignored() {
        let vsh = "#version 330\n#include <minecraft:projection.glsl>\n\
            uniform mat4 ProjMat;out vec4 vertexColor;\
            void main() { vertexColor = vec4(1.0); gl_Position = ProjMat[0]; }\n";
        let fsh = "#version 330\nin vec4 vertexColor;out vec4 fragColor;\
            void main() { fragColor = vertexColor; }\n";
        translate_core(vsh, fsh, &[], &include, &GLUE).unwrap();
    }

    /// A block declared on one line, as packs write a one-float uniform, is bound like one
    /// over several lines; the blocks and samplers the caller provides are accepted, in binding
    /// order, and one it doesn't provide is not.
    #[test]
    fn a_block_on_one_line_is_bound_and_provided_blocks_are_accepted() {
        let vsh = "#version 330\nlayout(std140) uniform Day { float daylight; };\nout vec4 vertexColor;\n\
            void main() { vertexColor = vec4(daylight); gl_Position = vec4(0.0); }\n";
        let fsh = "#version 330\nuniform sampler2D Bounds;\nin vec4 vertexColor;\nout vec4 fragColor;\n\
            void main() { fragColor = vertexColor * texture(Bounds, vec2(0.5)); }\n";
        let both = |name: &str| name == "Day" || name == "Bounds";
        let program = translate_core_with(vsh, fsh, &[], &include, &GLUE, &both).unwrap();
        assert_eq!(program.other, ["Day", "Bounds"]);
        // The bindings the caller gives them at.
        let bindings = |module: &naga::Module| -> Vec<(Option<String>, u32)> {
            module
                .global_variables
                .iter()
                .filter_map(|(_, g)| Some((g.name.clone(), g.binding.as_ref()?.binding)))
                .collect()
        };
        assert!(
            bindings(&program.vertex)
                .iter()
                .any(|(_, b)| *b == other_binding(0)),
            "{:?}",
            bindings(&program.vertex)
        );
        let fragment = bindings(&program.fragment);
        assert!(
            fragment.contains(&(Some("hz_tex_Bounds".into()), other_binding(1))),
            "{fragment:?}"
        );
        assert!(
            fragment.contains(&(Some("hz_smp_Bounds".into()), other_binding(1) + 1)),
            "{fragment:?}"
        );
        let error = translate_core(vsh, fsh, &[], &include, &GLUE).unwrap_err();
        assert!(error.contains("Day is not a block or sampler"), "{error}");
        let only_day = |name: &str| name == "Day";
        let error = translate_core_with(vsh, fsh, &[], &include, &GLUE, &only_day).unwrap_err();
        assert!(
            error.contains("Bounds is not a block or sampler"),
            "{error}"
        );
    }

    /// Only a uniform block's declaration names a block, written over several lines or on one.
    #[test]
    fn block_names_are_read_from_block_declarations_only() {
        assert_eq!(block_name("layout(std140) uniform Day {"), Some("Day"));
        assert_eq!(block_name("uniform Day { float daylight; };"), Some("Day"));
        assert_eq!(block_name("uniform Foo{"), Some("Foo"));
        assert_eq!(block_name("uniform Globals"), Some("Globals"));
        assert_eq!(block_name("uniform float x;"), None);
        assert_eq!(block_name("uniform sampler2D Sampler0;"), None);
    }

    #[test]
    fn what_horizon_does_not_provide_is_an_error() {
        let fsh = "#version 330\nin vec4 vertexColor;\nout vec4 fragColor;\nvoid main() { fragColor = vertexColor; }\n";
        // Another format's input is a global the shader may read: not an error.
        let normal = "#version 330\nin vec3 Normal;\nout vec4 vertexColor;\nvoid main() { vertexColor = vec4(Normal, 1.0); }\n";
        translate_core(normal, fsh, &[], &include, &GLUE).unwrap();
        let lightmap = "#version 330\nuniform sampler2D Lightmap;\nout vec4 vertexColor;\nvoid main() { vertexColor = texelFetch(Lightmap, ivec2(0), 0); }\n";
        let error = translate_core(lightmap, fsh, &[], &include, &GLUE).unwrap_err();
        assert!(
            error.contains("Lightmap is not a block or sampler"),
            "{error}"
        );
        // Behind a define the pipeline doesn't have, it is no error.
        let oit = "#version 330\n#ifdef OIT\nuniform sampler2D Coeff0;\n#endif\nout vec4 vertexColor;\n\
            void main() {\n#ifdef OIT\n    vertexColor = texelFetch(Coeff0, ivec2(0), 0);\n#endif\n}\n";
        translate_core(oit, fsh, &[], &include, &GLUE).unwrap();
        let missing = "#version 330\n#moj_import <minecraft:nothing.glsl>\nvoid main() {}\n";
        let error = translate_core(missing, fsh, &[], &include, &GLUE).unwrap_err();
        assert!(
            error.contains("minecraft:shaders/include/nothing.glsl not found"),
            "{error}"
        );
    }

    #[test]
    fn includes_resolve_by_id_or_from_the_including_folder() {
        assert_eq!(
            include_id("<fog.glsl>", "minecraft:shaders/core/").as_deref(),
            Some("minecraft:shaders/include/fog.glsl")
        );
        assert_eq!(
            include_id("<bitmade:hud/lanes.glsl>", "minecraft:shaders/core/").as_deref(),
            Some("bitmade:shaders/include/hud/lanes.glsl")
        );
        assert_eq!(
            include_id("\"../include/fog.glsl\"", "minecraft:shaders/core/").as_deref(),
            Some("minecraft:shaders/include/fog.glsl")
        );
    }
}
