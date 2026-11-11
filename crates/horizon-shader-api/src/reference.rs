//! docs/api/shaders-reference.md (0.1) and shaders-reference-0.2.md, generated from the stages
//! and inputs this crate defines (`tests/reference.rs` fails when a file drifts).
use crate::check::{MAX_EXPRESSIONS, MAX_TEXTURE_READS};
use crate::inputs::{DISPLACEMENT_INPUT, Frame, SURFACE, Struct};
use crate::manifest::{Api, MAX_GPU_MS, MAX_OPTIONS, MAX_POST_PASSES};
use crate::package::{MAX_TOTAL, MAX_WGSL};
use crate::stages::{GROUP, StageKind};

fn table(s: &Struct, offsets: Option<&[usize]>) -> String {
    let mut out = format!("### `{}`\n\n{}\n\n", s.name, one_line(s.doc));
    out += if offsets.is_some() {
        "| Field | Type | Offset | Meaning |\n| --- | --- | --- | --- |\n"
    } else {
        "| Field | Type | Meaning |\n| --- | --- | --- |\n"
    };
    for (i, f) in s.fields.iter().enumerate() {
        let offset = offsets.map_or(String::new(), |o| format!(" {} |", o[i]));
        out += &format!(
            "| `{}` | `{}` |{offset} {} |\n",
            f.name,
            f.ty,
            one_line(f.doc)
        );
    }
    out + "\n"
}

fn one_line(doc: &str) -> String {
    doc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where the line's reference lives, from the repository's root.
pub fn path(api: Api) -> &'static str {
    match api {
        Api::V0_1 => "docs/api/shaders-reference.md",
        Api::V0_2 => "docs/api/shaders-reference-0.2.md",
    }
}

/// 0.2's rules, after the introduction.
const RULES_0_2: &str = "\
**Experimental** ([stability.md](stability.md)): `horizon-shader check` takes 0.2 packs, the game draws them only
with `HORIZON_HDR_FRAME=float` set, the registry doesn't list them, and anything here may change in any release.
The design: [hdr-linear-light.md](../design/hdr-linear-light.md).

## Colour

Every colour a 0.2 stage reads or returns is **linear light**: BT.709 (sRGB) primaries by default, scene-referred,
1.0 is vanilla's white at paper white, more is brighter. A pack may ask for BT.2020 primaries, which hold colours
outside sRGB, with `[frame] primaries` (below). Horizon decodes vanilla's values with a 2.2 power (vanilla's, not
the piecewise sRGB curve), replacing negatives and NaN with 0 first, and encodes a stage's result the same way where
the frame stores gamma (`[frame] blend`, below).

- **Linear:** `MaterialInput` `albedo`, `tint`, `lightmap` and `colored_light`; `VertexInput.tint`; `SkyInput.color`;
  `WeatherInput.color`; `Frame` `sky_color` and `fog_color`; `block_texture`, `block_texture_level` and
  `block_light_color`; every colour texture a stage samples (`scene_color`, `previous`, `history`, the pack's targets,
  `opaque_color`, `reflection`, `shadow_color`).
- **Data, as in 0.1:** depth, normals, `light` levels, `emission`, every channel of the LabPBR maps, and the pack's
  `[[textures]]`.
- **`[[lights]]` and `[[particle_lights]]` colours** are written as in 0.1, in vanilla's gamma, and Horizon decodes
  them: `block_light_color` and `colored_light` are linear. `block_light_colors` holds them as written; read it through
  `block_light_color`.
- **Alpha is coverage only**, never light: brightness is in rgb alone. A post pass's alpha is ignored.

## Room to grow

`Frame`, the stage inputs (`SkyInput`, `VertexInput`, `MaterialInput`, `DisplacementInput`, `WeatherInput`,
`PostInput`, `TonemapInput`) and `surface_at`'s `SurfaceData` may gain fields in later releases. Read their fields and
pass them along, but don't construct them: `horizon-shader check` refuses `PostInput(...)` and the like. `Surface` is
yours to construct and keeps its one field; later material outputs come as helpers.

**New names reach a stage only if the pack asked for them.** A later release puts a new name before your files (a
helper, a struct, a resource, a constant) only under a manifest key the pack sets, or in a stage added later, whose
prelude is its own. A stage a pack has today keeps the names it has today, so a name of your own that is free now
stays yours: an addition doesn't break a pack that didn't ask for it. Names starting with `horizon_` are Horizon's
already, and new fields can't collide, since you don't construct those structs.

## The display

`frame.hdr`, `headroom`, `paper_white`, `peak` and `black_level` describe the display, read again every frame. With HDR
on, where the device keeps the HUD on a layer of its own, a 0.2 pack sees the display's: `hdr` 1, `paper_white` and
`peak` in nits, `headroom` the peak over paper white (at least 1) and `black_level` the display's black in nits.
Otherwise it sees SDR: `hdr` 0, `headroom` 1, `paper_white` and `peak` 80, `black_level` 0, and its picture ends on an
8-bit frame, clipped at white, before the camera overlays and the HUD draw. Horizon lifts the shadows onto
`black_level` after `[tonemap]`: read it, but don't apply it.

## `[frame] blend`

`blend = \"gamma\"` (the default) keeps the frame in vanilla's gamma, values above 1 kept: Horizon encodes the material,
sky and weather stages' results before they are stored and blended, so glass, water, clouds and particles blend as
vanilla's. `blend = \"linear\"` stores linear light, so see-through draws blend as light does. A stage sees the same
values either way. `\"linear\"` passes the check; the game doesn't serve it yet.

## `[frame] primaries`

`primaries = \"bt709\"` (the default) is the rule above. With `primaries = \"bt2020\"` every colour a stage reads or
returns is in BT.2020 primaries instead (D65 white, 1.0 still vanilla's white at paper white, values never negative).
It needs `blend = \"linear\"`, since a change of primaries is only right in linear light. `frame.primaries` says which
space the pack's colours are in, and `frame.luminance` holds that space's luminance weights: compute luminance as
`dot(color, frame.luminance)` and it is right in either. `\"bt2020\"` passes the check; the game doesn't serve it yet.

";

/// The line's reference.
pub fn markdown(api: Api) -> String {
    let mut out = format!(
        "# Shader API reference ({api})\n\n\
         Generated from `crates/horizon-shader-api` (`HORIZON_WRITE_DOCS=1 cargo test -p horizon-shader-api --test reference`\n\
         rewrites it; the test fails when it drifts). The API's design and pack format: [shaders.md](shaders.md).\n\n"
    );
    if api == Api::V0_2 {
        out += RULES_0_2;
    }
    out += "A stage is a function your WGSL defines, with the signature below; Horizon puts the prelude (these structs, the\n\
            resources and helpers) before your files and calls the function from its own pass. Your files declare no\n\
            bindings and no entry points, and no names starting with `horizon_`.\n\n\
            ## Stages\n\n\
            | Stage | In `horizon-shaders.toml` | Your function |\n| --- | --- | --- |\n";
    for &kind in api.stages() {
        let key = match kind {
            StageKind::Post => "[[post]]",
            StageKind::Sky => "[sky]",
            StageKind::Material => "[material]",
            StageKind::Vertex => "[vertex]",
            StageKind::Weather => "[weather]",
            StageKind::Tonemap => "[tonemap]",
        };
        out += &format!("| {} | `{key}` | `{}` |\n", kind.name(), kind.signature());
    }
    out += "\n";
    for &kind in api.stages() {
        out += &format!("## {}\n\n`{}`\n\n", kind.name(), kind.signature());
        out += &table(kind.input(), None);
        if kind == StageKind::Material {
            let surface = table(&SURFACE, None);
            // 0.2's lit colour is linear light and may pass white; 0.1's reference keeps its text.
            out += &match api {
                Api::V0_1 => surface,
                Api::V0_2 => surface.replace(
                    "The lit colour, 0..1, with its alpha.",
                    "The lit colour: rgb linear light, 0 or more and unbounded (more than 1 is brighter than \
                     white); alpha its coverage.",
                ),
            };
            out += "Under `[displacement]` the stage also defines \
                    `fn displacement(input: DisplacementInput) -> f32`, called for each corner of the grid the \
                    faces near the camera are cut into; the corner moves along the face's normal by the depth it \
                    gives at the point's height less the depth at the texture's mean height, pinned at the face's \
                    edges and easing out over the last quarter of the distance.\n\n";
            out += &table(&DISPLACEMENT_INPUT, None);
        }
        out += &format!(
            "Resources (bind group {GROUP}):\n\n| Binding | Declaration | Meaning |\n| --- | --- | --- |\n"
        );
        for r in kind.resources() {
            out += &format!("| {} | `{}` | {} |\n", r.binding, r.ty, r.doc);
        }
        out += "\n";
        if kind == StageKind::Tonemap {
            out += "It is a pure mapping from the scene's light to the output its `frame` describes, run once per pixel of\n\
                    each output, after the post chain and before the camera overlays and the HUD. Today the display is the\n\
                    only output; Horizon may add others for the same frame, such as an SDR screenshot or icon, where `frame`'s\n\
                    display fields describe SDR (`hdr` 0, `headroom` 1, `paper_white` and `peak` 80, `black_level` 0). Its\n\
                    only output is the colour it returns, so nothing carries from one call to the next. A pack without it gets\n\
                    Horizon's: a clip at white in SDR, and in HDR the curve Horizon uses without a pack, which keeps light up\n\
                    to 60% of the peak as it is and rolls it off into the peak above. Either runs once: Horizon doesn't map a pack's\n\
                    result again. Horizon holds the result to 0 to `frame.headroom`, then lifts the shadows onto\n\
                    `frame.black_level`.\n\n";
            continue;
        }
        if kind != StageKind::Sky {
            out += "Helpers:\n\n";
        }
        if kind == StageKind::Post {
            out += "- `fn depth_at(uv: vec2<f32>) -> f32`: the world's depth at `uv`.\n\
                    - `fn view_position(uv: vec2<f32>, depth: f32) -> vec3<f32>`: the camera-relative position, in blocks, of\n  \
                    the point at `uv` and `depth`.\n\
                    - `fn surface_at(texel: vec2<i32>) -> SurfaceData`: the solid terrain's surface at a pixel, as the \
                    material stage's `set_surface` left it: `present` (false where the sky, an entity, water, glass or a \
                    particle shows instead), `normal` (world axes), `smoothness` and `data`.\n";
        }
        if kind == StageKind::Material {
            out += "- `fn block_texture(uv: vec2<f32>) -> vec4<f32>`: the surface's block texture at `uv`, anywhere on \
                    the sprite, without the tint.\n\
                    - `fn block_texture_level(uv: vec2<f32>, lod: f32) -> vec4<f32>`: the same at mip level `lod`.\n\
                    - `fn block_texture_size() -> vec2<f32>`: its size in texels.\n\
                    - `fn block_texture_lod() -> f32`: the mip level `block_texture` reads at this point.\n";
            out += "- `fn block_pbr_maps() -> bool`: whether the resource packs ship LabPBR maps (`_n`, `_s`).\n\
                    - `fn block_normal_map(uv: vec2<f32>) -> vec4<f32>`: the surface's LabPBR normal map at `uv` (rg the \
                    normal along `input.tangent` and down the texture, b ambient occlusion, a height); (0.5, 0.5, 1, 1) \
                    without one.\n\
                    - `fn block_specular_map(uv: vec2<f32>) -> vec4<f32>`: its LabPBR specular map (r smoothness, g F0 or a \
                    metal, b porosity or subsurface, a emission, 1 none); (0, 0, 0, 0) without one.\n\
                    - `fn block_normal_map_level(uv: vec2<f32>, lod: f32) -> vec4<f32>`, \
                    `fn block_specular_map_level(uv: vec2<f32>, lod: f32) -> vec4<f32>`: the same at the block texture's mip level `lod`.\n\
                    - `fn block_pbr_size() -> vec2<f32>`: the LabPBR maps' size in texels.\n\
                    - `fn set_surface(normal: vec3<f32>, smoothness: f32, data: f32)`: what the post stage's \
                    `surface_at` reads for this point of solid terrain (`data` the pack's own; the face's normal and 0, 0 unset).\n";
        }
        if kind != StageKind::Sky {
            out += "- `fn shadow_position(position: vec3<f32>) -> vec3<f32>`: where the camera-relative point `position` is\n  \
                    stored in the shadow map, for `textureSampleCompareLevel`.\n\
                    - `fn shadow_distort(xy: vec2<f32>) -> vec2<f32>`: the map's distortion of a point at shadow clip `xy`.\n\n";
        }
        if kind == StageKind::Post {
            out += "A `[[post]]` entry with `output = \"<name>\"` writes a target of the pack's own, at its `scale` of the\n\
                    world's size (1, 0.5 or 0.25), instead of the next pass's `previous`; the prelude of every pass declares\n\
                    `var <name>: texture_2d<f32>` (group 3 from binding 32), which holds last frame's until written again (a\n\
                    pass reads a blank one in place of the target it writes).\n\n";
        }
    }
    out += "## The frame\n\n";
    let frame = Frame::line(api);
    out += &table(&frame, Some(&Frame::OFFSETS[..frame.fields.len()]));
    out += "## Anti-aliasing\n\n\
            While a pack's `[[post]]` chain draws the frame, the player's anti-aliasing settings do not run: `taa`, `fxaa`\n\
            and `upscaling` in `horizon-graphics.txt` have no effect, and the frame's projection is never jittered, because the\n\
            last post pass draws the picture itself and Horizon's own resolve, which does TAA, FXAA and TAA upscaling, is the\n\
            step it replaces. The world is still rendered at `resolutionScale` before the pack reads it.\n\n\
            A pack that wants smooth edges draws them itself, in a post pass: a spatial filter (FXAA, SMAA and the like) works on\n\
            `previous` as it is. A temporal pass cannot do what TAA does here, even though `history` and\n\
            `frame.previous_view_projection` give it the last frame: nothing moves the camera by a fraction of a texel from one\n\
            frame to the next, so a still camera gives it the same picture twice and there is no sub-pixel detail to gather.\n\n\
            Without a pack the settings apply as the player set them.\n\n";
    out += &format!(
        "## Options\n\n\
         Each `[[options]]` entry sets the WGSL `override` of the same name when the pipelines are built:\n\n\
         | `type` | Keys | The `override` |\n| --- | --- | --- |\n\
         | `bool` | `default` | `override <id>: bool` |\n\
         | `float` | `default`, `min`, `max`, `step` | `override <id>: f32` |\n\
         | `int` | `default`, `min`, `max` | `override <id>: i32` |\n\
         | `choice` | `values`, `default` (one of them) | `override <id>: u32`, the value's index |\n\n\
         ## Limits\n\n\
         - {MAX_POST_PASSES} post passes, {MAX_OPTIONS} options, `budget.gpu_ms` up to {MAX_GPU_MS}.\n\
         - {} KiB per WGSL file, {} MiB per pack.\n\
         - Per stage: {MAX_EXPRESSIONS} naga expressions over all its functions, {MAX_TEXTURE_READS} texture reads (samples,\n  \
         gathers and loads, the prelude's included).\n\n\
         ## The check\n\n\
         A pack runs only after it passes `horizon_shader_api::check` (the game when it loads a pack, the launcher before it\n\
         installs one, the registry's CI): each stage is parsed and validated by naga, checked against its signature and\n\
         the rules above, and translated for Vulkan (SPIR-V), Metal and DirectX 12 (HLSL) with its options at their\n\
         defaults, minimums and maximums. Every option must be an `override` of its type in some stage; an `override`\n\
         that is no option needs a default.\n",
        MAX_WGSL >> 10,
        MAX_TOTAL >> 20
    );
    out
}
