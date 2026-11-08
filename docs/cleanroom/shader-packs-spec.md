# Shader pack format: clean-room specification

> Published on 2026-11-08 as the record of the shader-pack clean room (#6739, #1555). The text is as written in
> October 2026, except that local paths, machine names and the names of private files are replaced by a
> description of what they were. Nothing else was changed.

Revision 1, 2026-10-24. Written by the spec team (an AI agent working for Ludovic) from public documentation only.
Process and rules: [README.md](README.md).

This document says what a "shader pack" (the OptiFine-style format of `shaderpacks/*.zip`) contains and what a game
must provide to run one. It is an interface description built from documentation for pack authors. It describes
no implementation: it does not say how any existing loader is built, and the implementer must not go and find out.

## 0. How to read this document

### 0.1 Source tags

Every statement carries a tag naming where it comes from. Section 10 lists each source with its URL, revision and licence.

| Tag | Source |
|---|---|
| `[OF-SH]` | OptiFine's `shaders.txt` (the format's original reference document) |
| `[OF-SP]` | OptiFine's sample `shaders.properties` (documents every key) |
| `[OF-PF]` | OptiFine's `properties_files.txt` (shared syntax of `.properties` files: block names, lists) |
| `[OF-TX]` | OptiFine's `texture.properties` (resource pack PBR format declaration) |
| `[OD]` | OptiDocs, a community rendering of the OptiFine documents |
| `[IR:page]` | The community shader documentation site `shaders.properties` (its "Reference" and "How To" pages) |
| `[SD:page]` | ShaderDoc, an older community document of the same format |
| `[SL:page]` | The shaderLABS wiki |
| `[GLSL]` | The OpenGL Shading Language 4.60.8 specification (includes the compatibility profile) |
| `[GLSL120]` | The OpenGL Shading Language 1.20 specification (`attribute`, `varying`, built-ins of `#version 120`) |
| `[GL]` | The OpenGL 4.6 compatibility-profile specification and the Khronos `gl.xml` registry (enum values) |
| `[LOGL]` | LearnOpenGL's normal-mapping article (general technique only) |
| `[PACK]` | What the `.properties` / `.lang` files and folder layout of three public packs show (BSL 10.1.8, Complementary Reimagined r5.9.3, Sildur's Vibrant 2.02 Extreme). Only key names and syntax are recorded, never their content |
| `[MC]` | Minecraft Java Edition 26.3 itself (the implementer reads it; this spec only points at it) |

Where two sources disagree, the spec says so and lists the case in section 9 (open questions).

### 0.2 Tiers

- **Tier A (base format):** documented by `[OF-SH]`/`[OF-SP]`, and usually restated by `[IR]`. Every pack assumes it.
- **Tier E (extension):** documented by `[IR]` as an extension of the base format ("exclusive" in its wording).
  Many current packs use Tier E features, often behind `#ifdef` checks; three of the four test packs request some.
- **Legacy:** documented as old or unused; accept it when parsing, implementing it is optional.

### 0.3 Words

"Must" means a pack written to the documentation breaks without it. "Should" means documented behaviour that a pack
may depend on. "Implementer decides" means the documentation is silent; see section 9.

### 0.4 Out of scope

Mod integrations described by `[IR:Mod Support/*]` (Distant Horizons `dh*` programs and keys, Voxy `voxy.json`,
Vivecraft, Photonics, Colorwheel `colorwheel.properties`), resource-pack custom skies and connected textures.
Packs that ship such files must still load; the extra files are ignored. `[PACK]` shows `voxy.json`,
`colorwheel.properties`, `pack.json` and `dh*` keys in real packs.

---

## 1. Pack layout

### 1.1 Container

- A pack is either a folder or a `.zip` archive placed in the game's shader pack folder. The pack's own root holds a
  folder named `shaders`; every path in this spec is relative to that `shaders` folder unless stated. `[OF-SH Shader Files]`, `[IR:overview]`
- Other files at the pack root (licence, readme) are ignored. `[IR:Guides/1_composite]`
- `[PACK]`: one pack's archive root contains several non-`shaders` folders and files (decoys such as
  `shaderFile_*.placebo`). The loader must look for the `shaders` folder by name and ignore everything else.

### 1.2 Dimension folders

- **Legacy rule:** shaders for one dimension can be put in `shaders/world<id>`, where `<id>` is the legacy numeric
  dimension id: `world0` the Overworld, `world-1` the Nether, `world1` the End. `[OF-SP]`, `[IR:Misc/dimension_properties]`
- When the folder for the current dimension exists, programs are loaded only from it and the base `shaders` folder is
  ignored for that dimension. An empty dimension folder therefore turns shaders off in that dimension. `[OF-SP]`
- `[IR]` states a stronger rule: if any `worldN` folder exists, programs load only from `worldN` folders. See OQ-12.
- `[OF-SP]` says only `.vsh` and `.fsh` files are loaded from a dimension folder; `[IR]` lists more stage files. See OQ-12.
- Dimension folders are also scanned for options (section 8). Options there may use different names. `[OF-SP]`
- **`dimension.properties` (Tier E):** `dimension.<folder> = <dimension id> ...` maps any folder name to dimensions by
  their namespaced id (`minecraft:the_nether`); `*` matches every dimension not listed elsewhere. Dimensions not
  listed (and no `*`) use the base `shaders` folder. `[IR:Misc/dimension_properties]`
  `[PACK]` shows old dimension ids (`minecraft:nether`) listed next to current ones, so unknown ids must be ignored.
- Program names elsewhere (`program.<name>.enabled`, `!program.<name>` in profiles, `alphaTest.<name>`) may carry a
  dimension folder prefix, as in `world-1/composite2`. `[OF-SP]`, `[IR:ordering]`

### 1.3 Program files and stages

A program is named (section 2) and made of stage files `shaders/<program>.<ext>`: `[OF-SH Shader Files]`, `[IR:Programs/overview]`

| Extension | Stage | Tier |
|---|---|---|
| `.vsh` | vertex | A |
| `.gsh` | geometry | A (needs GL 3.2 layout qualifiers, or the old `GL_ARB_geometry_shader4` extension with `const int maxVerticesOut`) `[OF-SH]` |
| `.fsh` | fragment | A |
| `.csh` | compute | A for composite-style passes (`[OF-SH Compute Shaders]`); E for `setup` |
| `.tcs`, `.tes` | tessellation control / evaluation | E (gbuffers-style only, triangles only, needs the `TESSELLATION_SHADERS` flag) `[IR:Programs/overview]` |

- Compute shaders attached to a program are named `<program>.csh` and `<program>_a.csh` ... `<program>_z.csh`
  (27 at most). They run before the program's vertex stage, in alphabetical order starting with the unsuffixed one. `[OF-SH]`, `[IR:Programs/overview]`
- A compute shader needs `#version 430` or later and a `layout(local_size_x=..., ...) in;` declaration. `[OF-SH]`
- The work-group count comes from `const ivec3 workGroups = ivec3(x,y,z);` (fixed) or
  `const vec2 workGroupsRender = vec2(sx,sy);` (relative to the render size). Default: `workGroupsRender = vec2(1,1)`,
  one invocation per pixel; the count is `ceil(viewSize * workGroupsRender / localSize.xy)` with z = 1. `[OF-SH]`, `[IR:Constants/workGroupsRender]`
  Tier E adds `indirect.<pass>` (section 3.2).
- Other files a pack may hold: `shaders.properties` (section 3), `block.properties`, `item.properties`,
  `entity.properties` (section 4), `dimension.properties` (1.2), `lang/<locale>.lang` (section 8), textures and data files
  referred to by `shaders.properties`, each optionally with a `<file>.mcmeta` (section 6.6), and files only reached by
  `#include` (any extension; `[PACK]` shows `.glsl`). `[OF-SP]`, `[IR]`

### 1.4 `#include`

- `#include "<path>"` in a stage file is replaced by the contents of that file. `[OF-SP]`, `[IR:Misc/include]`
- A path with a leading `/` is relative to the `shaders` folder; any other path is relative to the folder of the file
  that contains the directive. `[OF-SP]`, `[IR:Misc/include]`
- Included files may include others; the documented depth limit is 10. `[OF-SP]`
- Nothing prevents including a file twice; packs guard with `#ifndef X / #define X / #endif`. `[OF-SP]`
- `#include` is resolved before (and independently of) the GLSL preprocessor's conditionals: it is a text inclusion.
  Whether an `#include` inside a false `#if` block is still read is not documented. OQ-14.
- `#include` is not allowed in `shaders.properties`. `[IR:Shaders.Properties/overview]`
- Error line numbers: the documentation only says they may not match the pack's files. OQ-15.

### 1.5 The preprocessor

Stage files use the standard GLSL preprocessor `[GLSL §3.3]`: `#define` / `#undef` (with or without parameters),
`#if #ifdef #ifndef #elif #else #endif`, `defined`, the integer operators of the GLSL preprocessor table
(unary `+ - ~ !`, `* / %`, `+ -`, `<< >>`, relational, equality, `& ^ |`, `&& ||`), token pasting `##`,
`#error`, `#pragma`, `#extension`, `#version`, `#line`, and the macros `__LINE__ __FILE__ __VERSION__`.
Names starting `GL_` are reserved. Expressions in `#if` work on integer literals only.

The loader itself also reads preprocessor lines in two places, before compilation:

1. **Configuration scanning.** The loader's own scan for configuration (const directives, comment directives, sampler
   use, options) obeys the conditional directives, so a `const` inside a false `#ifdef` does not count. `[OF-SH]` lists the
   forms it recognises: `#define`, `#undef`, `#ifdef`, `#ifndef`, `#if <int>`, `#if defined X`, `#if !defined X`, the
   same three for `#elif`, `#else`, `#endif`. Option macros (section 8) and the standard macros are defined during this scan.
2. **Properties files.** `shaders.properties`, `block.properties`, `item.properties`, `entity.properties` may use
   conditional directives with the standard macros and option macros. `[OF-SP]`, `[OF-SH Block ID mapping]`
   In `shaders.properties`, `#define` and `#include` are not allowed. `[IR:Shaders.Properties/overview]`
   `[OF-SP]` adds that `version`, `oldLighting`, `separateAo`, `sliders`, `profiles` and `screen` are parsed without option macros.
   `[PACK]` shows `#if MC_VERSION >= 11300`, `#if <option> == 1`, `#ifdef <option>`, `#elif <option> == 128` and
   option names used as *values* (for example a buffer size or an image size given as an option name), so the
   implementer must also substitute option macros inside property values. OQ-16.

#### 1.5.1 Standard macros

Inserted right after the `#version` line of every stage file `[OF-SH Standard Macros]`. Values as documented:

| Macro | Meaning | Source |
|---|---|---|
| `MC_VERSION` | Minecraft version as `major*10000 + minor*100 + patch` (1.21.0 → 12100, 1.7.10 → 10710) | `[OF-SH A]`, `[IR:Macros/MC_VERSION]`; for 26.3 see OQ-1 |
| `MC_GL_VERSION` | highest GL version, decimal point removed (330, 460) | `[OF-SH B]`, `[IR]` |
| `MC_GLSL_VERSION` | highest GLSL version (120, 330, 460) | `[OF-SH C]`, `[IR]` |
| one of `MC_OS_WINDOWS`, `MC_OS_MAC`, `MC_OS_LINUX`, `MC_OS_OTHER` | operating system | `[OF-SH D]` |
| one of `MC_GL_VENDOR_AMD`, `_ATI`, `_INTEL`, `_MESA`, `_NVIDIA`, `_XORG`, `_OTHER` | driver vendor | `[OF-SH E]` |
| one of `MC_GL_RENDERER_RADEON`, `_GEFORCE`, `_QUADRO`, `_INTEL`, `_GALLIUM`, `_MESA`, `_OTHER` (`_APPLE`: E) | GPU family | `[OF-SH F]`, `[IR:Macros/GPU_Hardware]` |
| `MC_<extension>` (e.g. `MC_GL_ARB_shader_texture_lod`) | the GL extension is supported; only added for names the pack references | `[OF-SH G]` |
| `MC_FXAA_LEVEL` (2 or 4) | when FXAA is on | `[OF-SH H]` |
| `MC_NORMAL_MAP`, `MC_SPECULAR_MAP` | when normal / specular maps are enabled | `[OF-SH H]` |
| `MC_RENDER_QUALITY`, `MC_SHADOW_QUALITY` | 0.5, 0.70710677, 1.0, 1.4142135 or 2.0 | `[OF-SH H]` |
| `MC_HAND_DEPTH` | 0.0625, 0.125 or 0.25: depth multiplier of hand geometry (applied through its projection matrix) | `[OF-SH H]`, `[IR:Macros/MC_HAND_DEPTH]` |
| `MC_OLD_HAND_LIGHT`, `MC_OLD_LIGHTING` | when those options are on | `[OF-SH H]` |
| `MC_ANISOTROPIC_FILTERING <level>` | when anisotropic filtering is on (then `spriteBounds` is set) | `[OF-SH H]` |
| `MC_TEXTURE_FORMAT_LAB_PBR`, `MC_TEXTURE_FORMAT_LAB_PBR_1_3` | the resource pack declares `format=lab-pbr` / `lab-pbr/1.3` in its `optifine/texture.properties` | `[OF-SH I]`, `[OF-TX]`, `[IR:Macros/MC_TEXTURE_FORMAT_LAB_PBR]` |
| `MC_RENDER_STAGE_*` (24 names, section 5.4) | constants compared with `renderStage` | `[OF-SH J]`, `[IR:Macros/Render_Stages]` |
| `MC_MIPMAP_LEVEL` | current maximum mipmap level | E `[IR]` |
| `MAX_COLOR_BUFFERS` | how many colortex buffers exist | E `[IR]` |
| `IRIS_FEATURE_<flag>` | an optional feature flag (section 3.2) is supported | E `[IR:flags]` |
| `IS_IRIS`, `IRIS_VERSION`, `IRIS_TAG_SUPPORT`, `IRIS_HAS_TRANSLUCENCY_SORTING`, `IRIS_HAS_CONNECTED_TEXTURES` | identify a specific loader | E; whether to define them is a product decision, OQ-2 |

Option macros (section 8) are also defined, with the values the player chose.

### 1.6 `#version` and the two GLSL dialects

- The first non-comment line of a stage file should be `#version <n> [profile]`. No directive means GLSL 1.10.
  A profile word (`core`, `compatibility`) is allowed from 150; without it, 150+ means `core`. `[GLSL §3.3]`
- Packs are written at every version from 120 to 460, in both profiles. The documentation recommends
  `compatibility` and says both behave the same in modern loaders. `[IR:How To/compatibility_vs_core]`, `[IR:Guides/0_intro]`
- `[SL:Compiler_Behavior_Notes]`: some drivers reject `#include` or `#extension` lines placed before `#version`, so the
  loader keeps `#version` first, then its injected macros, then the pack's `#extension` lines.
- **Compatibility built-ins the game must supply** (used by most packs) `[GLSL §7.2, §7.4.1, §7.1.7, §8.9.5]`:
  vertex inputs `gl_Vertex`, `gl_Color`, `gl_Normal`, `gl_MultiTexCoord0..7` (`gl_MultiTexCoord1` and `2` both carry the
  lightmap coordinate `[IR:Attributes/gl_MultiTexCoord1]`), `gl_SecondaryColor`, `gl_FogCoord`;
  uniform state `gl_ModelViewMatrix`, `gl_ProjectionMatrix`, `gl_ModelViewProjectionMatrix`, `gl_NormalMatrix`,
  `gl_TextureMatrix[]` and their `Inverse` / `Transpose` / `InverseTranspose` forms, `gl_Fog` (OQ-28);
  `ftransform()`; fragment outputs `gl_FragColor` and `gl_FragData[]`; varyings `gl_TexCoord[]`, `gl_FrontColor`,
  `gl_FogFragCoord`; the `attribute`/`varying` keywords of GLSL 1.10–1.20 `[GLSL120]`; the old texture functions (`texture2D`, `texture2DLod`,
  `texture3D`, `shadow2D`, `texture2DProj`...).
- **Core-profile names** (Tier A since Minecraft 1.17, `[OF-SH Attributes/Uniforms]`): attributes `vaPosition`,
  `vaColor`, `vaUV0`, `vaUV1` (overlay), `vaUV2` (lightmap), `vaNormal`; uniforms `modelViewMatrix`,
  `modelViewMatrixInverse`, `projectionMatrix`, `projectionMatrixInverse`, `textureMatrix`, `normalMatrix`,
  `colorModulator`, `modelOffset` (`chunkOffset` in `[IR]`; offer both, OQ-27), `alphaTestRef`. For terrain, `vaPosition`
  is relative to the chunk and `modelOffset` must be added. The lightmap matrix for `vaUV2` is the documented constant
  `TEXTURE_MATRIX_2`: scale 1/256 (0.00390625) on x, y, z and translation 1/32 (0.03125) on x, y, z. `[OF-SH Constants]`, `[IR:Attributes/gl_MultiTexCoord1]`
- Fragment outputs: index n of the output list (section 6.1.4) is written through `gl_FragData[n]`, through
  `out vec4 outColor<n>` (GLSL 130+), or through `layout(location = n) out` (330+). `[OF-SH Color Attachments]`, `[IR:Constants/rendertargets]`
- `[IR:Misc/patcher]` warns that its implementation reserves identifiers starting `iris_`, `irisMain`, `moj_import`;
  packs avoid them. Horizon may reserve its own prefix in the same way.
- Packs rely on driver leniency that the GLSL spec does not allow (implicit int→float casts, non-constant `const`,
  constant arguments used in constant expressions, functions called at global scope). `[SL:Compiler_Behavior_Notes]` lists them per vendor. OQ-29.

### 1.7 Directives written in comments and constants

The loader reads these from the stage files (after include expansion and configuration preprocessing, 1.5):

| Directive | Effect | Source |
|---|---|---|
| `/* DRAWBUFFERS:XYZ */` | output list as single digits 0–9, e.g. `0257` → colortex0, 2, 5, 7 | `[OF-SH]`, `[IR:Constants/drawbuffers]` |
| `/* RENDERTARGETS: a,b,c */` | output list as comma-separated indices 0–15 (more with Tier E) | `[OF-SH]`, `[IR:Constants/rendertargets]` |
| `/* SHADOWRES:n */`, `/* SHADOWFOV:f */`, `/* SHADOWHPL:f */`, `/* WETNESSHL:f */`, `/* DRYNESSHL:f */` | legacy forms of `shadowMapResolution`, `shadowMapFov`, `shadowDistance`, `wetnessHalflife`, `drynessHalflife` | `[OF-SH]` |
| `/* GAUX4FORMAT:<fmt> */` with RGBA32F, RGB32F or RGB16 | legacy format of colortex7 | `[OF-SH]` |
| `const <type> <name> = <value>;` | configuration constants of section 6 and 7 | `[OF-SH Fragment Shader Configuration]` |
| `uniform <type> <sampler name>;` | declaring a sampler turns on the buffer behind it (6.1.1, 7) | `[OF-SH]` |
| `in vec3 mc_Entity;`, `in vec2 mc_midTexCoord;`, `in vec4 at_tangent;` | the loader must feed these attributes | `[OF-SH Vertex Shader Configuration]` |
| `const int countInstances = n;` | draw the geometry n times, setting `instanceId` (Legacy) | `[OF-SH]`, `[SD:const-directives]` |

- `RENDERTARGETS`/`DRAWBUFFERS` must be a block comment, ideally alone on its line, and may sit anywhere in the `.fsh`. `[IR:Constants/rendertargets]`
- A `<buffer>Format` constant uses names that are not GLSL identifiers, so packs put it in a block comment; the loader
  reads it there. It should be alone on its line, never in a `//` comment. `[IR:Constants/buffer_format]`, `[SL:Rendering_Pipeline]`
- Where the scan looks: `[IR:Constants/overview]` says most constants count from any one file, "usually only fragment
  shaders are scanned"; `[SD:const-directives]` lists fragment shaders only; `[IR]` per-constant pages say "any GLSL file"
  for most and name specific programs for a few (`shadowNearPlane`, `voxelDistance`, `entityShadowDistanceMul`:
  composite, deferred, final, prepare). OQ-17.

---

## 2. Programs and the order they run in

### 2.1 Frame order

One frame with a pack loaded runs these passes in this order `[IR:Programs/overview]`, `[OF-SH Overview, Shader Programs]`, `[SL:Rendering_Pipeline]`:

| # | Pass | Kind | Tier | Writes |
|---|---|---|---|---|
| 0 | `setup`, `setup1`..`setup99` | compute only, once after load and on every resize | E | images, SSBOs |
| 1 | `begin`, `begin1`..`begin99` | composite-style | E | colortex |
| 2 | `shadow` (and the `shadow_*` variants, 2.4) | gbuffers-style, from the light | A | shadowtex0/1, shadowcolor0/1 |
| 3 | `shadowcomp`, `shadowcomp1`..`99` | composite-style | A | shadowcolor |
| 4 | `prepare`, `prepare1`..`99` | composite-style | A | colortex |
| 5 | gbuffers, opaque part | gbuffers-style | A | colortex, depth |
| 6 | `deferred_pre` (virtual: flips only), `deferred`, `deferred1`..`99` | composite-style | A | colortex |
| 7 | gbuffers, translucent part | gbuffers-style | A | colortex, depth |
| 8 | `composite_pre` (virtual), `composite`, `composite1`..`99` | composite-style | A | colortex |
| 9 | `final` | composite-style | A | the screen |
| 10 | GUI and menus | no program | A | the screen |

- Numbered programs run in numeric order: no suffix, then 1, 2, ... 99. Gaps are allowed. `[OF-SH]`, `[IR]`
- Mipmaps requested for shadow buffers are generated after pass 2 and before pass 3. `[IR:Constants/shadow_mipmaps]`
- The order of gbuffers draws inside 5 and 7 is not fixed by the format; it follows the game's own draw order. `[IR:Programs/overview]`
  The documents only fix which geometry is before or after `deferred`:
  - By default only `gbuffers_water`, `gbuffers_weather` and `gbuffers_hand_water` draw after deferred. `[IR:Programs/gbuffers]`
  - `particles.before.deferred` (A) and `particles.ordering` (E) move particles; `separateEntityDraws` (E) moves translucent
    entities and block entities after deferred. Section 3.2.
  - Depth snapshots tie the rest down: `depthtex1` excludes translucent geometry and first holds meaningful data in
    `deferred`; `depthtex2` also excludes hand geometry, so the opaque hand draws after the `depthtex2` snapshot and
    before the `depthtex1` snapshot. `[OF-SH Depth buffers usage]`, `[IR:Buffers/depthtex]`
  - `[SL:Rendering_Pipeline]` describes the usual opaque order as sky (skybasic, then skytextured), terrain, entities and
    block entities, particles; then deferred; then weather and water.
- Which vanilla 26.3 draw goes to which program is decided by the geometry column of 2.3 against `[MC]`'s own render
  passes (sky, terrain layers, entities, block entities, particles, weather, clouds, world border, block outline, hand).
- `F3+R` and the command `/reloadShaders` reload the pack. `[OF-SH]`

### 2.2 Program kinds

- **Gbuffers-style** (`gbuffers_*`, `shadow*`): draw the world's geometry. Vertex and fragment stages required; geometry
  (A) and tessellation (E) optional; no compute. Only these programs see vertex attributes. `[IR:Programs/overview]`
- **Composite-style** (`begin`, `shadowcomp`, `prepare`, `deferred`, `composite`, `final`): draw one quad that covers the
  output. Vertex and fragment required, geometry optional; compute shaders optional and run first. With compute shaders
  and no vertex/fragment stage, only the compute shaders run. `[IR:Programs/overview]`, `[IR:ordering]`
  - The quad: `gl_ModelViewMatrix` is the identity in these programs `[IR:Uniforms/matrices]`; packs transform the vertex
    with `ftransform()` and read `(gl_TextureMatrix[0] * gl_MultiTexCoord0).xy` as a coordinate from (0,0) at the bottom
    left to (1,1) at the top right `[IR:Guides/1_composite]`, `[SD:passes/composite]`. Exact vertex positions and the
    projection matrix are not documented: OQ-6.
  - It covers the resolution of the buffers the program writes to `[IR:Programs/*]`; `final` always covers the window
    whatever the buffer sizes or scale. `[IR:Programs/final]`, `[SL:Rendering_Pipeline]`
- **Compute-only**: `setup` (E).

### 2.3 Gbuffers programs and their fallbacks

When a program's files are missing (or it is disabled, 2.5), its geometry is drawn with the program in the fallback
column, recursively. `[OF-SH Shader Programs]`, `[IR:Programs/gbuffers]`

| Program | Geometry (as documented) | Fallback | Notes |
|---|---|---|---|
| `gbuffers_basic` | leash, block selection box, debug chunk borders | none | `[OF-SH]`, `[IR]` |
| `gbuffers_line` | block outline, fishing line | `gbuffers_basic` | `[OF-SH]`, `[IR]` |
| `gbuffers_textured` | particles (unlit) | `gbuffers_basic` | `[IR]` also lists the world border here |
| `gbuffers_textured_lit` | lit particles (most particles) | `gbuffers_textured` | `[OF-SH]` lists the world border here. OQ-4 |
| `gbuffers_skybasic` | sky colour / horizon, stars, void | `gbuffers_basic` | |
| `gbuffers_skytextured` | sun, moon | `gbuffers_textured` | |
| `gbuffers_clouds` | vanilla clouds | none `[OF-SH]` / `gbuffers_textured` `[IR]`, `[SL]` | OQ-3 |
| `gbuffers_terrain` | terrain blocks: solid and cutout | `gbuffers_textured_lit` | |
| `gbuffers_terrain_solid`, `gbuffers_terrain_cutout` | Legacy "not used" in `[OF-SH]`; E: solid only / cutout only | `gbuffers_terrain` | |
| `gbuffers_damagedblock` | block-breaking overlay | `gbuffers_terrain` | |
| `gbuffers_block` | block entities, sign text | `gbuffers_terrain` | |
| `gbuffers_beaconbeam` | beacon beam | `gbuffers_textured` | |
| `gbuffers_item` | Legacy "not used" | `gbuffers_textured_lit` | `[OF-SH]` |
| `gbuffers_entities` | entities, name tag text | `gbuffers_textured_lit` | |
| `gbuffers_entities_glowing` | entities with the glowing (spectral) effect | `gbuffers_entities` | A in `[OF-SH]`; `[IR]` does not recognise it. OQ-4 |
| `gbuffers_armor_glint` | enchantment glint on armour and held items | `gbuffers_textured` | |
| `gbuffers_spidereyes` | eyes of spider, enderman, dragon | `gbuffers_textured` | |
| `gbuffers_hand` | the hand and opaque held objects | `gbuffers_textured_lit` | |
| `gbuffers_weather` | rain and snow (not particles) | `gbuffers_textured_lit` | after deferred |
| `gbuffers_water` | translucent terrain (water, stained glass) | `gbuffers_terrain` | after deferred |
| `gbuffers_hand_water` | translucent held objects | `gbuffers_hand` | after deferred |
| E `gbuffers_particles` | particles | `gbuffers_textured_lit` | `[IR]` |
| E `gbuffers_particles_translucent` | translucent particles (with `particles.ordering=mixed`) | `gbuffers_particles` | |
| E `gbuffers_block_translucent` | translucent block entities | `gbuffers_block` | |
| E `gbuffers_entities_translucent` | translucent entities | `gbuffers_entities` | |
| E `gbuffers_lightning` | lightning, ender dragon death beams (their depth goes through `gbuffers_basic`) | `gbuffers_entities` | |

- Blocks can be moved between terrain layers with `layer.*` in `block.properties` (section 4.2).
- `[OF-SH]` lists "gui, menus" under the program name `<none>`: GUI is never drawn by a pack program.

### 2.4 Shadow programs

- `shadow` draws everything the shadow pass includes, from the light's point of view, into the shadow buffers.
  Geometry type is told apart with `renderStage`. `[OF-SH]`, `[IR:Programs/shadow]`
- `shadow_solid`, `shadow_cutout`: Legacy "not used", fall back to `shadow`. `[OF-SH]`
- E: `shadow_solid`, `shadow_cutout`, `shadow_water`, `shadow_entities` (entities and beacon beam), `shadow_block` (block
  entities); each falls back to `shadow`, and with no `shadow` program "a default shader is used". `[IR:Programs/shadow]`
- What the shadow pass includes is controlled by `shadowTerrain`, `shadowTranslucent`, `shadowEntities`,
  `shadowBlockEntities` (A) and `shadowPlayer`, `shadowLightBlockEntities` (E). Section 3.2.

### 2.5 Enabling, disabling and missing programs

- A program is defined when its required stage files exist in the active folder (1.2).
- `program.<name>.enabled=<expr>` (section 3.3) and `!program.<name>` in the active profile (section 8.4) disable a
  program. A disabled program is treated exactly as a missing one, so its fallback is used. `[OF-SP]`
- A missing composite-style program (or a missing number in a series) is skipped.
- A gbuffers program whose fallback chain ends without a program: `[OF-SH]` gives "none"; `[IR:Guides/2_gbuffers]` says a
  built-in program is used. What that built-in draws is OQ-5.
- No `final` program: OQ-5.
- **When the shadow pass runs:** it runs when some program uses a shadow buffer, which is detected from the sampler
  declarations of 6.3 (`shadow`, `watershadow`, `shadowtex0/1`, `shadowcolor*`). `[OF-SH Fragment Shader Configuration]`, `[IR:ordering]`
  E: `shadow.enabled=true|false` forces it on or off.
- A pack with no program at all for a dimension (empty `worldN` folder) renders that dimension as vanilla. `[OF-SP]`

### 2.6 Compute passes, barriers and concurrency

- Compute shaders of one composite-style program run one after another, before its vertex stage.
- Base behaviour: every texture read and image load/store sees the results of every previous compute or composite pass,
  including earlier compute shaders of the same program (a full memory barrier around each dispatch). `[IR:ordering]`
- E `allowConcurrentCompute=true`: the compute shaders of one program may run at the same time; a barrier is only
  guaranteed after the last of them. Passes of different programs stay in order. `[IR:ordering]`
- Compute shaders write colortex and shadowcolor buffers only through images (6.5). `[IR:Programs/overview]`

---

## 3. `shaders.properties`

### 3.1 Syntax

- Lives at `shaders/shaders.properties`. Lines `key=value`; `#` starts a comment line; blank lines allowed; key order does
  not matter; keys are case-sensitive. `[OF-PF Overview]`, `[IR:Shaders.Properties/overview]`
- Spaces around `=` are allowed `[IR]` examples, `[PACK]`. Lines may be indented `[PACK]`.
- A value may continue on the next line when a line ends with `\` `[PACK]` (Java properties style). OQ-16.
- Preprocessor lines (`#if`, `#ifdef`, `#ifndef`, `#elif`, `#else`, `#endif`) are evaluated, not treated as comments
  (1.5, item 2). A `#` followed by text that is not a directive is a comment. `[PACK]` shows comment lines that start
  with `#` directly followed by a word (for example a commented-out key), so the loader must tell directives from
  comments by the directive word.
- Lists inside values are separated by spaces.

### 3.2 Keys

Every key the documents describe. "Default" is the documented default; "—" means none documented.

#### Pack and feature flags

| Key | Value | Meaning | Default | Tier / source |
|---|---|---|---|---|
| `version.<mc_version>` | OptiFine edition letter+digit (`D1`) | minimum OptiFine version per Minecraft version; parse and ignore | — | A `[OF-SP]` |
| `iris.features.required` | flag list | the pack needs these features: refuse to load if one is unsupported, and enable them | — | E `[IR:flags]` |
| `iris.features.optional` | flag list | enable those supported and define `IRIS_FEATURE_<flag>` for each | — | E `[IR:flags]` |

Feature flags `[IR:flags]` (✔ = must be requested before the feature may be used): `BLOCK_EMISSION_ATTRIBUTE`,
`CAN_DISABLE_WEATHER`, `COMPUTE_SHADERS`, `CUSTOM_IMAGES` ✔, `ENTITY_TRANSLUCENT`, `HIGHER_SHADOWCOLOR` ✔,
`PER_BUFFER_BLENDING`, `REVERSED_CULLING`, `SEPARATE_HARDWARE_SAMPLERS` ✔, `SSBO` ✔, `TESSELATION_SHADERS` /
`TESSELLATION_SHADERS` ✔, `FADE_VARIABLE` ✔, `TEXTURE_FILTERING` ✔. An unknown flag in `required` refuses the pack;
in `optional` it is ignored.

#### Program order and content

| Key | Value | Meaning | Default | Tier / source |
|---|---|---|---|---|
| `program.<program>.enabled` | boolean expression (3.3) | disable a program when false | true | A `[OF-SP]` |
| `particles.before.deferred` | bool | all particles before deferred | false (after) | A `[OF-SP]`, `[IR:ordering]` |
| `particles.ordering` | `mixed`, `after`, `before` | mixed = opaque particles before deferred, translucent after; overrides the previous key | `after` if any deferred program exists, else `mixed` | E `[IR:ordering]` |
| `separateEntityDraws` | bool | translucent entities / block entities after deferred (in the `*_translucent` programs, else their fallbacks) | false | E `[IR:ordering]` |
| `shadow.enabled` | bool | force the shadow pass on or off | detected (2.5) | E |
| `shadowTerrain` | bool | solid and cutout terrain in the shadow pass | true | A `[OF-SP]` |
| `shadowTranslucent` | bool | translucent terrain in the shadow pass | true | A |
| `shadowEntities` | bool | entities in the shadow pass | true `[IR]` | A |
| `shadowBlockEntities` | bool | block entities in the shadow pass | true | A |
| `shadowPlayer` | bool | the player (and what it rides) in the shadow pass; forced on when `shadowEntities` is on | — | E |
| `shadowLightBlockEntities` | bool | with `shadowBlockEntities=false`, still draw light-emitting block entities | false | E |
| `skipAllRendering` | bool | skip most gbuffers geometry (terrain, entities, block entities); shadows and post-processing still run | false | E |
| `voxelizeLightBlocks` | bool | draw each invisible light block as one degenerate quad at its centre, with its id and light level in the lightmap coordinate | false | E `[IR:ordering]` |
| `indirect.<pass>` | `<ssbo index> <byte offset>` | read the compute dispatch size (a `uvec3`) from an SSBO | — | E |
| `allowConcurrentCompute` | bool | 2.6 | false | E |

Defaults marked "true" for the shadow-content keys are not stated by `[OF-SP]`; `[IR]` states `shadowEntities` true. OQ-19.

#### Features and options

| Key | Value | Meaning | Default | Tier / source |
|---|---|---|---|---|
| `clouds` | `fast`, `fancy`, `off` | overrides the player's cloud setting (`[OF-SP]`: the video setting wins) | player's choice | A `[OF-SP]`, `[IR:features]`; precedence OQ-20 |
| `moon`, `sun` | bool | draw the moon / sun texture | on (implied, not stated) | A |
| `sky`, `stars` | bool | draw the vanilla sky (disc, horizon, void) / the stars | on (implied) | E |
| `underwaterOverlay` | bool | the water overlay when the camera is under water | on (implied) | A |
| `vignette` | bool | vanilla vignette | on (implied) | A |
| `oldHandLight` | bool | `heldBlockLightValue` (and `heldItemId`) take the hand with the higher light value; the player can override | true `[IR]`, false `[SD]` | A; OQ-21 |
| `dynamicHandLight` | bool | dynamic hand light of a "dynamic lights" feature; off if the pack does its own | true | A `[OF-SP]`, `[SD:directives/properties]` |
| `oldLighting` | bool | apply vanilla's per-face brightness to the vertex colour; the player can override | false | A |
| `separateAo` | bool | vanilla AO goes to `color.a` instead of being multiplied into `color.rgb` (terrain) | false | A `[OF-SP]`, `[IR:features]` |
| `supportsColorCorrection` | bool | the pack converts to the display colour space itself (`currentColorSpace`) | false | E |
| `weather` | `<geometry> <splashes>` (two bools) | rain/snow geometry and rain splash particles | `true true` | E (needs `CAN_DISABLE_WEATHER`) |
| `endFlashShadows` | bool | in the End, the shadow light follows the End flash (`shadowLightPosition = endFlashPosition`) | false | E |

#### Rendering state

| Key | Value | Meaning | Tier / source |
|---|---|---|---|
| `alphaTest.<program>` | `off` or `<func> <ref>`; func ∈ `NEVER LESS EQUAL LEQUAL GREATER NOTEQUAL GEQUAL GL_ALWAYS` | alpha test of a gbuffers program. With `gl_FragColor`/`gl_FragData` output the loader applies it to output 0; with `out` variables only `alphaTestRef` is passed and the pack discards itself | A `[OF-SP]`, `[IR:rendering]`, `[IR:Uniforms/rendering]` |
| `blend.<program>` | `off` or `<src> <dst> <srcA> <dstA>` | blend factors of a gbuffers program; factors: `ZERO ONE SRC_COLOR ONE_MINUS_SRC_COLOR DST_COLOR ONE_MINUS_DST_COLOR SRC_ALPHA ONE_MINUS_SRC_ALPHA DST_ALPHA ONE_MINUS_DST_ALPHA SRC_ALPHA_SATURATE` | A |
| `blend.<program>.<buffer>` | same | per output buffer (`colortex5`...); others keep the program's | A (`PER_BUFFER_BLENDING` E queries support) |
| `scale.<program>` | `<s>` or `<s> <ox> <oy>`, each 0..1 | viewport of a composite-style program within its output: only the region is drawn, buffer size unchanged | A `[OF-SP]`, `[IR:rendering]`; `[SD]` says offsets unsupported, OQ-22 |
| `flip.<program>.<buffer>` | bool | force or suppress the main/alt flip of that buffer after that program (6.1.6) | A |
| `size.buffer.<buffer>` | `<w> <h>`: integers = pixels, decimals = fraction of the render size | fixed size of a colortex buffer; gbuffers programs cannot render to it; one program renders only to buffers of one size | A `[OF-SP]`, `[IR:rendering]` |
| `backFace.solid`, `backFace.cutout`, `backFace.cutoutMipped`, `backFace.translucent` | bool | `[OF-SP]`: true renders back faces of that terrain layer (default false); `[IR]` words it as turning back-face culling on or off. OQ-38 | A (`cutoutMipped`: `[IR]`) |
| `frustum.culling` | bool | frustum culling of the main view | A, default true |
| `occlusion.culling` | bool | the game's occlusion culling of the main view | E, default true |
| `shadow.culling` | `true`, `false`, `reversed` | culling of shadow-pass geometry against what can shadow the view; `reversed` (E) keeps everything within `voxelDistance` and culls beyond | A (+E) |
| `rain.depth` | bool | rain/snow write depth (into `depthtex0` only, like translucents) | A, default false |
| `beacon.beam.depth` | bool | the opaque inner beam writes depth (all depth buffers) | A, default false |

The default blend for translucent geometry is `SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA` `[IR:rendering]`.
Composite-style programs draw with blending disabled `[OF-SH Color Attachments]`. Other per-program defaults: OQ-23.

#### Custom uniforms

| Key | Meaning | Tier |
|---|---|---|
| `uniform.<type>.<name>=<expr>` | a value computed on the CPU and sent to every program as `uniform <type> <name>` | A |
| `variable.<type>.<name>=<expr>` | the same, not sent; usable in later expressions | A |

`<type>` ∈ `float int bool vec2 vec3 vec4`. Section 3.4.

#### Textures and buffers (details in 6.5, 6.6)

| Key | Tier |
|---|---|
| `texture.<stage>.<sampler>=<path>` (and the raw form, and suffixes `.0`–`.9` on `<sampler>` to bind several types) | A |
| `texture.noise=<path>` | A |
| `customTexture.<name>=<path>` (and raw form) | E |
| `image.<name>=...` | E (`CUSTOM_IMAGES`) |
| `bufferObject.<index>=...` | E (`SSBO`) |

#### Settings screen (details in section 8)

`screen`, `screen.<NAME>`, `screen.columns`, `screen.<NAME>.columns`, `sliders`, `profile.<NAME>`. A `[OF-SP]`.

#### Keys seen in packs but not documented

`breaksAnisotropy` `[PACK]`, and the mod-support keys of 0.4. OQ-24.

### 3.3 Program-enable expressions

`program.<program>.enabled` takes a boolean expression over boolean (switch) options, with `!`, `&&`, `||`, brackets, and
the literals `true` / `false`. Example form: `(A || B) && !C`. `[OF-SP]`, `[IR:ordering]`. The same expression language
as 3.4 can parse it.

### 3.4 Custom uniform expressions

#### Evaluation

- Uniforms and variables are evaluated on the CPU in file order and are refreshed "on program change", that is before each
  program is used with current values. `[OF-SP]`, `[IR:custom_uniforms]`
- A uniform can be read by every program. Variables are only visible to later expressions.
- `random()` and friends give a new value each frame. `[IR:custom_uniforms]`
- `smooth()` keeps state between frames per `id` (generated when omitted; ids must be unique). `[OF-SP]`

#### Grammar

The documents give the elements, not a grammar or precedence. The following is the spec's formalisation; the precedence
follows the C/GLSL convention, which the documents imply by using GLSL-like syntax (OQ-25):

```
expr     := or
or       := and ( "||" and )*
and      := equal ( "&&" equal )*
equal    := rel ( ("==" | "!=") rel )*
rel      := add ( ("<" | "<=" | ">" | ">=") add )*
add      := mul ( ("+" | "-") mul )*
mul      := unary ( ("*" | "/" | "%") unary )*
unary    := ("-" | "!") unary | postfix
postfix  := primary ( "." component )*
primary  := number | "true" | "false" | "pi" | name | call | "(" expr ")"
call     := name "(" [ expr ( "," expr )* ] ")"
component:= "x"|"y"|"z"|"w"|"r"|"g"|"b"|"a"|"s"|"t"|"p"|"q"|"0"|"1"|"2"|"3"   (vectors; one component, no swizzles)
          | digit "." digit                                                      (matrices: row.column, from 0)
number   := decimal literal, optional fraction and exponent
```

- Components may only follow a name (a uniform or variable), not an arbitrary sub-expression. `[IR:custom_uniforms]`
- `%` is the remainder; `fmod(x,y)` is the floored modulus `x - y*floor(x/y)`. `[OF-SP]`, `[IR]`
- Names: built-in scalar and vector uniforms of section 5 (not the dynamic ones: `entityColor`, `entityId`,
  `blockEntityId`, `fogMode`, `fogColor`, `chunkOffset`/`modelOffset`), earlier variables and uniforms, the biome
  parameters and constants below, and the player booleans. Existing matrix uniforms can be read element by element;
  new matrices cannot be made. `[OF-SP]`, `[IR:custom_uniforms]`
- Values convert between int, float and bool as needed; the result is converted to the declared type. The exact
  conversion rules are OQ-25.

#### Functions

Union of `[OF-SP]` and `[IR:custom_uniforms]` (`[IR]`-only names are Tier E):

| Function | Meaning |
|---|---|
| `sin cos tan asin acos atan(x)` | trigonometry, radians |
| `atan2(y,x)`, E `atan(y,x)` | two-argument arctangent |
| `torad(deg)`, `todeg(rad)`; E `radians`, `degrees` | angle conversion |
| `min(x,y,...)`, `max(x,y,...)` | any number of arguments `[OF-SP]` |
| `clamp(x,min,max)` | |
| `abs floor ceil exp frac log sqrt round signum`; E `sign exp2 exp10 log2 log10 log(base,x)` | |
| `pow(x,y)` | |
| `fmod(x,y)` | floored modulus |
| `lerp(k,x,y)` `[OF-SP]` / E `mix(x,y,a)` | linear interpolation (note the argument orders differ) |
| E `edge(k,x)` | 0 if x < k else 1 |
| `random()`; E `random(min,max)`, `randomInt()`, `randomInt(min,max)` | per-frame random value; ranges as `[IR]` states (max exclusive) |
| `if(c1,v1,[c2,v2,...],else)` | first value whose condition holds |
| `ifb(c1,v1,...,else)` | the boolean variant `[OF-SP]` |
| `smooth([id,] val, [fadeIn, [fadeOut]])` | exponential smoothing of `val`; fade defaults 1 second `[OF-SP]` / 1 tick half-life `[IR]` (OQ-26) |
| `print(id, n, x)` | log `x` every n-th frame `[OF-SP]`; may be a no-op |
| `between(x,min,max)` | inclusive |
| `equals(a,b,eps)` | `abs(a-b) <= eps` |
| `in(x, v1, v2, ...)` | x equals one of them |
| `vec2(..) vec3(..) vec4(..)` | constructors |

Functions accept int or float and return the same type unless stated; `abs floor ceil min max clamp smooth` also work
component-wise on vectors. `[IR:custom_uniforms]`

#### Parameters only for expressions (Tier A) and as uniforms (Tier E)

| Name | Type | Meaning | Source |
|---|---|---|---|
| `biome` | int (float in `[OF-SP]`) | biome at the player, compared with `BIOME_<NAME>` constants | `[OF-SP]`, `[IR:Uniforms/biome]` |
| `biome_category` | int | 0..16 in this order: `CAT_NONE CAT_TAIGA CAT_EXTREME_HILLS CAT_JUNGLE CAT_MESA CAT_PLAINS CAT_SAVANNA CAT_ICY CAT_THE_END CAT_BEACH CAT_FOREST CAT_OCEAN CAT_DESERT CAT_RIVER CAT_SWAMP CAT_MUSHROOM CAT_NETHER` | `[OF-SP]` |
| `biome_precipitation` | int | 0 `PPT_NONE`, 1 `PPT_RAIN`, 2 `PPT_SNOW` | `[OF-SP]` |
| `temperature` | float | biome temperature (`[OF-SP]`: 0..1; `[IR]`: vanilla range −0.7..2.0) | |
| `rainfall` | float | biome downfall (humidity), 0..1 | |
| `is_alive is_burning is_child is_glowing is_hurt is_in_lava is_in_water is_invisible is_on_ground is_ridden is_riding is_sneaking is_sprinting is_wet` | bool | state of the player (the camera entity) | `[OF-SP]`; `[IR]` also exposes six of them as uniforms |

- `[OF-SP]` documents the rain/snow rule: precipitation falls where `biome_precipitation != PPT_NONE`, rain if
  `temperature >= 0.15`, otherwise snow.
- `BIOME_<NAME>` constants exist for every biome `[OF-SP]`; `[IR]` examples use current biome names
  (`BIOME_MANGROVE_SWAMP`, `BIOME_OLD_GROWTH_PINE_TAIGA`) and legacy ones (`BIOME_EXTREME_HILLS`). `[PACK]` also compares
  `biome` with plain numbers. The numbering is OQ-11.
- `[IR]` exposes `biome`, `biome_category`, `biome_precipitation`, `temperature`, `rainfall` and the player booleans as
  normal uniforms too (Tier E).

---

## 4. ID mapping files

### 4.1 `block.properties`, `item.properties`, `entity.properties`

| File | Line form | Feeds | Source |
|---|---|---|---|
| `shaders/block.properties` | `block.<id>=<block> <block> ...` | `mc_Entity.x`, `blockEntityId`, E `currentSelectedBlockId`, E `currentRenderedItemId` for block items | `[OF-SH Block ID mapping]`, `[IR:Misc/block_properties]` |
| `shaders/item.properties` | `item.<id>=<item> ...` | `heldItemId`, `heldItemId2`, E `currentRenderedItemId` | `[OF-SH]`, `[IR:Misc/item_properties]` |
| `shaders/entity.properties` | `entity.<id>=<entity> ...` | `entityId`, E `vehicleId` | `[OF-SH]`, `[IR:Misc/entity_properties]` |

- `<id>` is an integer. One line per id `[OF-SH]`; one name belongs to one id `[IR]`.
- Names: `[namespace:]name`, default namespace `minecraft`. Blocks may add state filters
  `:<property>=<v1>,<v2>...` repeated: values in one filter are alternatives, separate filters must all match. `[OF-PF Blocks]`, `[IR:Misc/block_properties]`
- Unknown names (removed blocks, other mods) are ignored silently: `[PACK]` lists pre-1.13 names next to current ones.
- The files use conditional directives with standard and option macros (1.5). `[OF-SH]`
- E: `%<tag>` selects every block of a block tag; individual entries win over tags when both match. `[IR:Misc/block_properties]`, `[IR:Macros/IRIS_TAG_SUPPORT]`
- E names that are not registry entries: in `entity.properties` `minecraft:name_tag`, `minecraft:entity_shadow`,
  `minecraft:entity_flame`, `minecraft:zombie_villager_converting`, `minecraft:current_player`,
  `minecraft:end_crystal_beam`; in `item.properties` `trim_<material>`, `minecraft:player_cape`, `minecraft:elytra_with_cape`. `[IR]`
- Value ranges and the value for "not in the file": section 5.3 and OQ-9.
- Mods may ship `assets/<modid>/shaders/block.properties` (and item, entity) in their own files `[OF-SH]`; Horizon's
  sandboxed mods are out of scope for revision 1.
- Without `block.properties`, ids follow the pre-1.13 numeric block ids (Legacy). `[IR:Attributes/mc_Entity]`

### 4.2 Render layers

In `block.properties`: `layer.solid=<blocks>`, `layer.cutout=<blocks>`, E `layer.cutout_mipped=<blocks>`,
`layer.translucent=<blocks>` move blocks to another terrain layer. Solid: no alpha, no blending; cutout: alpha test, with
mipmaps; translucent: blending, mipmaps (drawn by `gbuffers_water`). `[OF-SH Block render layers]`, `[IR:Misc/block_properties]`
Full opaque cubes cannot move, because culling, AO and light would change. `[OF-SH]`

---

## 5. Interface: attributes, uniforms, samplers

Update classes used below:

- **F** set once per frame (same value in every program of the frame);
- **P** set per program or pass (differs between gbuffers, shadow and composite-style programs);
- **D** "dynamic": may change between draws within one program (`[OF-SP]`, `[SD:uniforms]`); not usable in custom uniforms;
- **S** smoothed over time with a documented half-life (section 5.6);
- **C** constant for the life of a loaded pack.

"View space" is the camera's eye space (right-handed GL convention, looking down −z). "Player space" has the camera at
the origin and world-aligned axes. "World space" is player space plus `cameraPosition`. Shadow spaces are the same
from the light. `[IR:How To/coordinate_spaces]`, `[SL:Coordinate_Spaces]` (cited by `[IR]`).

### 5.1 Vertex attributes (gbuffers-style programs)

| Attribute | Type | Meaning | Programs | Source |
|---|---|---|---|---|
| `gl_Vertex` / `vaPosition` | vec4 / vec3 | position in model space, which varies by geometry: always go through the model-view matrix. Terrain `vaPosition` is relative to the chunk origin (add `modelOffset`) | all `.vsh` | `[OF-SH]`, `[IR:Attributes/gl_Vertex]` |
| `gl_Color` / `vaColor` | vec4 | vertex colour: biome tint, vanilla AO (in rgb, or in a with `separateAo`), old lighting if on; alpha carries tint alpha for some non-terrain geometry | gbuffers, shadow | `[IR:Attributes/gl_Color]` |
| `gl_MultiTexCoord0` / `vaUV0` | vec4 / vec2 | atlas coordinate; apply `gl_TextureMatrix[0]` / `textureMatrix` (glint scrolls through it). In composite-style programs: the quad coordinate | all | `[IR:Attributes/gl_MultiTexCoord0]` |
| `gl_MultiTexCoord1` (= `gl_MultiTexCoord2`) / `vaUV2` | vec4 / ivec2 | lightmap coordinate: x = block light, y = sky light, currently 0–240 in steps of 16; apply `gl_TextureMatrix[1]` / `TEXTURE_MATRIX_2` to get 0..1 lightmap texture coordinates | gbuffers, shadow | `[IR:Attributes/gl_MultiTexCoord1]`, `[OF-SH]` |
| `vaUV1` | ivec2 | entity overlay coordinate (packs should prefer `entityColor`) | gbuffers, shadow | `[OF-SH]`, `[IR:Attributes/vaUV1]` |
| `gl_Normal` / `vaNormal` | vec3 | normal in model space, not guaranteed normalised; to view space with `gl_NormalMatrix` | gbuffers, shadow | `[IR:Attributes/gl_Normal]` |
| `mc_Entity` | vec2/vec3/vec4 as declared | x = block id from `block.properties` (5.3); y = render type: 1 for fluids, −1 otherwise `[IR]` | `gbuffers_terrain`, `gbuffers_water`, `shadow` (+ variants) | `[OF-SH]`, `[IR:Attributes/mc_Entity]`; OQ-9 |
| `mc_midTexCoord` | vec2 (often declared vec4) | atlas coordinate of the centre of this quad's texture area (average of the quad's four coordinates) | gbuffers, shadow | `[OF-SH]`, `[IR:Attributes/mc_midTexCoord]`, `[SD:vertex-format-extensions]` |
| `at_tangent` | vec4 | xyz: tangent in model space (approximate, not normalised); w: handedness sign, non-zero, in [−1,1], so `bitangent = cross(tangent, normal) * sign(w)` | gbuffers, shadow | `[OF-SH]`, `[IR:Attributes/at_tangent]`; the per-triangle tangent from positions and UVs is the standard technique of `[LOGL]` |
| `at_midBlock` | vec4 (vec3 in `[OF-SH]`) | xyz: offset from the vertex to the block centre, in 1/64 block; E w: the block's own emitted light 0–15 | terrain, water, shadow | `[OF-SH]`, `[IR:Attributes/at_midBlock]` |
| `at_velocity` | vec3 | vertex offset since the previous frame, view space; only entities and block entities | gbuffers | `[OF-SH]` (A); `[SD:unsupported-features]` says another loader lacks it |
| E `mc_chunkFade` | float, declared by the loader | 0..1 fade-in of the chunk (Minecraft's chunk fade) in terrain; `-1.0` constant elsewhere | gbuffers | `[IR:Attributes/mc_chunkFade]` (needs `FADE_VARIABLE`) |

Declaring `mc_Entity`, `mc_midTexCoord` or `at_tangent` is how a pack asks for them `[OF-SH Vertex Shader Configuration]`.

### 5.2 Uniforms

Types and meanings are from `[OF-SH Uniforms]` unless another tag is given. E rows come from `[IR:Uniforms/*]`.

#### Camera and player

| Uniform | Type | Meaning, space, units | Upd | Tier |
|---|---|---|---|---|
| `cameraPosition` | vec3 | camera position, world space, blocks. Wrapped for precision after large distances (OQ-10) | F | A |
| `previousCameraPosition` | vec3 | last frame's `cameraPosition` (0 on the first frame `[SD]`) | F | A |
| `cameraPositionInt`, `cameraPositionFract` (+ `previous...`) | ivec3, vec3 | integer part and [0,1) fraction of the unwrapped camera position | F | E |
| `eyeAltitude` | float | Y of the camera entity, blocks (as on the F3 screen) | F | A |
| `eyePosition` | vec3 | world-space position of the player model's head (differs from the camera in third person) | F | E |
| `relativeEyePosition` | vec3 | `cameraPosition - eyePosition` | F | E |
| `playerLookVector`, `playerBodyVector` | vec3 | world-aligned direction of the player model's head / body (`[IR]`: body currently equals look) | F | E |
| `vehicleLookVector`, `relativeVehiclePosition` | vec3 | direction of the ridden entity; offset from it to the camera | F | E |
| `upPosition` | vec3 | world up in view space, length 100 (equals `gbufferModelView[1].xyz * 100`) | F | A |
| `eyeBrightness` | ivec2 | light at the camera: x block, y sky; light level 0–15 → 0–240 | F | A |
| `eyeBrightnessSmooth` | ivec2 | `eyeBrightness` smoothed, half-life `eyeBrightnessHalflife` (10 ticks) | S | A |
| `centerDepthSmooth` | float | `depthtex0` at the screen centre, smoothed with `centerDepthHalflife` (1 tick); computed only if a program declares it | S | A |
| `firstPersonCamera` | bool | first-person view | F | E |
| `isEyeInWater` | int | 0 none, 1 water, 2 lava, 3 powder snow | F | A |
| `blindness`, `nightVision` | float | effect strength 0..1 (fractional only during transitions) | F | A |
| `darknessFactor` | float | darkness effect strength 0..1 | F | A (1.19+) |
| `darknessLightFactor` | float | the pulsing light dimming of the darkness effect 0..1 | F | A |
| `playerMood` | float | cave-ambience mood 0..1, resets to 0 when it reaches 1 | F | A |
| `constantMood` | float | the same without the reset | F | E |
| `heldItemId`, `heldItemId2` | int | main / off hand item id from `item.properties` (5.3). With `oldHandLight`, `heldItemId` takes the hand with the brighter item `[SD]` | F | A |
| `heldBlockLightValue`, `heldBlockLightValue2` | int | light level of the held item's block (0–15 in vanilla), main / off hand. With `oldHandLight`, the first takes the higher of the two | F | A |
| `hideGUI` | int `[OF-SH]` / bool `[IR]` | GUI hidden (F1) | F | A; OQ-9 |
| `screenBrightness` | float | brightness option 0..1 | F | A |
| `isSpectator`, `isRightHanded`, `isRiding`, `isElytraFlying`, `inSwimmingAnimation`, `feetInWater`, `vehicleInWater` | bool | as named | F | E |
| `currentPlayerHealth/Air/Armor/Hunger` | float | fraction 0..1; −1 in creative and spectator | F | E |
| `maxPlayerHealth/Air/Armor/Hunger` | float | maximum (air 300, armour 50, hunger 20 in vanilla; health and air −1 in creative/spectator) | F | E |
| `is_burning is_hurt is_invisible is_on_ground is_sneaking is_sprinting` | bool | as in 3.4 | F | E (A only inside expressions) |

#### Time and system

| Uniform | Type | Meaning | Upd | Tier |
|---|---|---|---|---|
| `worldTime` | int | day time in ticks, `dayTime % 24000` (0–23999) | F | A |
| `worldDay` | int | `dayTime / 24000` | F | A |
| `moonPhase` | int | 0–7: full, waning gibbous, third quarter, waning crescent, new, waxing crescent, first quarter, waxing gibbous | F | A |
| `frameCounter` | int | frame index, 0 to 720719 then back to 0 | F | A |
| `frameTime` | float | previous frame's duration, seconds | F | A |
| `frameTimeCounter` | float | run time in seconds, wraps to 0 after 3600 s, keeps running while paused `[IR]` | F | A |
| `viewWidth`, `viewHeight` | float | size of the render buffers in pixels (the window times the render-quality factor `[SD]`) | F | A |
| `aspectRatio` | float | `viewWidth / viewHeight` | F | A |
| `currentColorSpace` | int | 0 sRGB, 1 DCI-P3, 2 Display P3, 3 Rec. 2020, 4 Adobe RGB | F | E |
| `currentDate`, `currentTime`, `currentYearTime` | ivec3, ivec3, ivec2 | OS date (y,m,d), time (h,m,s, 24 h), seconds elapsed / left in the year | F | E |

#### Sky, weather, dimension

| Uniform | Type | Meaning | Upd | Tier |
|---|---|---|---|---|
| `sunPosition`, `moonPosition` | vec3 | position in view space, length 100: (0,100,0) and (0,−100,0) transformed by the model-view matrix the game uses to draw the sun and moon `[SD]` | F | A |
| `shadowLightPosition` | vec3 | the sun by day, the moon by night (the light used for the shadow pass) | F | A |
| `sunAngle` | float | 0 sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight. Equals vanilla's celestial angle (0 = noon) plus 0.25, wrapped into [0,1) `[SD]` | F | A |
| `shadowAngle` | float | `sunAngle` by day; `sunAngle − 0.5` by night (0..0.5) `[SD]`, `[IR]`; `[OF-SH]` says 0..1 | F | A |
| `rainStrength` | float | rain level 0..1 (vanilla's interpolated rain level) | F | A |
| `wetness` | float | `rainStrength` smoothed with `wetnessHalflife` (600 ticks) when rising and `drynessHalflife` (200 ticks) when falling | S | A |
| `thunderStrength` | float | thunder level 0..1 | F | E |
| `lightningBoltPosition` | vec4 | player-space position of one rendered lightning bolt, w = 1; zero if none | F | E |
| `fogColor` | vec3 | vanilla horizon/fog colour (sRGB values) | D | A |
| `skyColor` | vec3 | vanilla upper sky colour | F | A |
| `fogMode` | int | `GL_LINEAR` 9729, `GL_EXP` 2048, `GL_EXP2` 2049 `[GL]` | D | A |
| `fogStart`, `fogEnd` | float | vanilla fog distances, blocks | F (OQ-36) | A |
| `fogDensity` | float | relative density 0..1 (`[SD]`: dynamic; vanilla lava goes above 1) | D | A |
| `fogShape` | int | 0 sphere, 1 cylinder | F (OQ-36) | A |
| `bossBattle` | int | 1 custom, 2 ender dragon, 3 wither, 4 raid | F | A `[OF-SH]` (`[SD]`: not copied elsewhere) |
| `endFlashPosition`, `endFlashIntensity`, `previousEndFlashIntensity` | vec3, float, float | the End flash: view-space position (length 100), intensity 0..1 | F | E |
| `ambientLight`, `bedrockLevel`, `cloudHeight` (NaN without clouds), `hasCeiling`, `hasSkylight`, `heightLimit`, `logicalHeightLimit` | | dimension-type values of the current dimension | F | E |
| `biome`, `biome_category`, `biome_precipitation`, `temperature`, `rainfall` | | 3.4 | F | E (A in expressions) |

#### Rendering state

| Uniform | Type | Meaning | Upd | Tier |
|---|---|---|---|---|
| `near` | float | near plane, 0.05 | C | A |
| `far` | float | render distance in blocks (not the real far plane, which is about 4×) | F | A |
| `renderStage` | int | the `MC_RENDER_STAGE_*` of the current draw (5.4) | D | A |
| `entityColor` | vec4 | entity overlay: rgb colour, a blend factor (hurt flash red, creeper white flash); 0 when no entity | D | A |
| `entityId` | int | id of the drawn entity from `entity.properties` | D | A |
| `blockEntityId` | int | id of the drawn block entity's block from `block.properties` | D | A |
| `blendFunc` | ivec4 | the current blend factors (srcRGB, dstRGB, srcA, dstA) as GL enum values (`GL_ZERO` 0, `GL_ONE` 1, `GL_SRC_COLOR` 768 ... `GL_SRC_ALPHA_SATURATE` 776) `[GL]` | D | A |
| `alphaTestRef` | float | alpha below which the pack should discard (commonly 0.1; follows `alphaTest.<program>`) | P/D | A |
| `atlasSize` | ivec2 | size of the bound texture atlas, 0 when none | D | A |
| `spriteBounds` | vec4 | atlas bounds (u0,v0,u1,v1) of the sprite, only with anisotropic filtering | D | A |
| `instanceId` | int | instance index with `countInstances > 1` (0 = original) | D | Legacy |
| `terrainTextureSize`, `terrainIconSize` | ivec2, int | "not used"; supply 0 | C | Legacy |
| `colorModulator` | vec4 | the colour multiplier of the current draw (core profile) | D | A |
| `modelOffset` / `chunkOffset` | vec3 | terrain chunk offset for `vaPosition` (also clouds `[OF-SH]`); 0 when not drawing terrain | D | A |
| `textureFilteringMode` | int | 0 nearest, 1 RGSS, 2 anisotropic | F | E (`TEXTURE_FILTERING`) |
| E `currentRenderedItemId` | int | id of the item or armour being drawn | D | E |
| E `currentSelectedBlockId`, `currentSelectedBlockPos` | int, vec3 | targeted block's id; player-space centre (−256 each when none) | F | E |
| E `vehicleId` | int | `entity.properties` id of the ridden entity | F | E |

#### Matrices

| Uniform | Meaning | Upd | Tier |
|---|---|---|---|
| `gbufferModelView` | player space → view space for this frame's camera (after camera setup, before drawing the world) | F | A |
| `gbufferModelViewInverse` | its inverse (computed on the CPU) | F | A |
| `gbufferProjection` | view → clip for the world (not the hand's, which scales depth by `MC_HAND_DEPTH`) | F | A |
| `gbufferProjectionInverse` | its inverse | F | A |
| `gbufferPreviousModelView`, `gbufferPreviousProjection` | last frame's values (zeros on the first frame `[SD]`) | F | A |
| `shadowModelView`, `shadowModelViewInverse` | player space → shadow view space, and back | F | A |
| `shadowProjection`, `shadowProjectionInverse` | shadow view → shadow clip, and back | F | A |
| `gl_ModelViewMatrix` (`modelViewMatrix`), `gl_ProjectionMatrix` (`projectionMatrix`), their inverses, `gl_ModelViewProjectionMatrix`, `gl_NormalMatrix` (`normalMatrix`) | the current draw's model → view → clip; identity model-view in composite-style programs; in the shadow pass the shadow matrices | D | A `[GLSL §7.4.1]`, `[IR:Uniforms/matrices]` |
| `gl_TextureMatrix[0]` (`textureMatrix`) | atlas coordinate transform (identity except e.g. glint scrolling) | D | A |
| `gl_TextureMatrix[1]` | lightmap coordinate → 0..1 texture coordinate (5.1) | D | A |

#### Samplers (names a program may declare)

The texture unit numbers below are those `[OF-SH]` documents; they matter only to a GL implementation, but they also
show which names alias the same texture.

Gbuffers and shadow programs `[OF-SH GBuffers/Shadow Uniforms]`:

| Unit | Name (aliases) | Content |
|---|---|---|
| 0 | `gtexture` (`texture` in GLSL 120, `tex` in shadow) | the bound atlas or texture of the draw |
| 1 | `lightmap` | vanilla lightmap texture (the light colour table indexed by block and sky light) |
| 2 | `normals` | normal atlas built from the resource pack's `_n` textures; missing → (127,127,255,255)/255 `[IR:Buffers/atlases]` |
| 3 | `specular` | specular atlas from `_s` textures; missing → (0,0,0,0) `[IR]` |
| 4 | `shadowtex0` (`watershadow`; `shadow` when no `watershadow` is declared) | shadow depth, everything |
| 5 | `shadowtex1` (`shadow` when `watershadow` is declared) | shadow depth without translucent geometry |
| 6 | `depthtex0` (gbuffers only) | scene depth so far |
| 7–10 | `colortex4`–`colortex7` (`gaux1`–`gaux4`) | custom textures, or the output of deferred programs (for the translucent gbuffers) |
| 11/12 | `depthtex1` (gbuffers: 12 in the texture table, 11 in the uniform table) | OQ-30 |
| 13, 14 | `shadowcolor0` (`shadowcolor`), `shadowcolor1` | shadow colour buffers |
| 15 | `noisetex` | noise texture |
| 16–23 | `colortex8`–`colortex15` | as 7–10 |

- `[IR:Buffers/colortex]`: in gbuffers programs `colortex0`–`3` cannot be read (that name samples the atlas). An undeclared
  sampler name falling back to the atlas is undefined behaviour `[IR:Buffers/atlases]`.

Composite-style programs `[OF-SH Composite and Deferred Uniforms]`:

| Unit | Name (aliases) |
|---|---|
| 0–3 | `colortex0`–`3` (`gcolor`, `gdepth`, `gnormal`, `composite`) |
| 4, 5 | `shadowtex0`, `shadowtex1` (`shadow`, `watershadow` as above) |
| 6 | `depthtex0` (`gdepthtex`) |
| 7–10 | `colortex4`–`7` (`gaux1`–`gaux4`) |
| 11, 12 | `depthtex1`, `depthtex2` |
| 13, 14 | `shadowcolor0` (`shadowcolor`), `shadowcolor1` |
| 15 | `noisetex` |
| 16–23 | `colortex8`–`15` |

Tier E adds `colortex16`–`31` `[IR:Buffers/colortex]`, `shadowcolor2`–`7` (`HIGHER_SHADOWCOLOR`), `shadowtex0HW`,
`shadowtex1HW` (`SEPARATE_HARDWARE_SAMPLERS`, 6.3), `customTexture.*` and `image.*` samplers (6.5).

Images: `colorimg0`–`5` and `shadowcolorimg0`–`1` (A: `[OF-SH Image access]`, "all programs"; E: `colorimgN` for every
colortex). Declared as `layout(<format>) uniform image2D <name>;` and used with `imageLoad`/`imageStore`.

Sampler types: `sampler2D` for float/normalised formats, `isampler2D`/`usampler2D` for integer formats `[SL:Rendering_Pipeline]`,
`sampler2DShadow` with hardware shadow filtering `[IR:Buffers/shadowtex]`, `sampler1D/3D/2DRect` for raw custom textures `[OF-SP]`.

### 5.3 ID values

| Value | Not in the file | No file | Range | Source |
|---|---|---|---|---|
| `mc_Entity.x` | −1 `[IR]` (older readers expected 65535) | legacy numeric id | signed 16-bit `[IR]`, wider in E | `[IR:Attributes/mc_Entity]` |
| `blockEntityId` | 65535 `[IR:Uniforms/id]` | 0 | the index lists [−32768, 32767] | OQ-9 |
| `entityId` | 65535 | 0 | 0–65535 | `[IR:Uniforms/id]` |
| `heldItemId(2)` | −1 (also when holding nothing) | 0 | `[IR]` index lists 0–65535 | `[IR]`, `[SD]` |
| `currentSelectedBlockId` | 0 | 0 | | E |

### 5.4 Render stages

`renderStage` takes one of these macros, listed in the order the stages draw `[OF-SH J]`, `[IR:Macros/Render_Stages]`:
`NONE, SKY, SUNSET, CUSTOM_SKY, SUN, MOON, STARS, VOID, TERRAIN_SOLID, TERRAIN_CUTOUT_MIPPED, TERRAIN_CUTOUT, ENTITIES,
BLOCK_ENTITIES, DESTROY, OUTLINE, DEBUG, HAND_SOLID, TERRAIN_TRANSLUCENT, TRIPWIRE, PARTICLES, CLOUDS, RAIN_SNOW,
WORLD_BORDER, HAND_TRANSLUCENT` (each prefixed `MC_RENDER_STAGE_`). `TERRAIN_CUTOUT_MIPPED` is listed by `[IR]` only.
Values are integer constants; the documents do not give them (OQ-7). `[IR]` notes that cutout terrain is reported as
`TERRAIN_SOLID` there; packs should not depend on the split.

### 5.5 GL conventions the game must reproduce

Packs are GLSL written for OpenGL, so their math assumes GL conventions. The game must present the same numbers
whatever its own graphics API does internally `[GL]`, `[GLSL]`:

- Clip space z runs from −1 to 1; window depth is `z_ndc * 0.5 + 0.5`; depth buffers clear to 1.0 and store that
  window depth (`[IR:Macros/MC_HAND_DEPTH]` shows packs undoing it with `depth * 2 − 1`).
- Window and texture coordinates start at the bottom left: `gl_FragCoord` (0,0) is the bottom-left pixel; texture
  coordinate (0,0) is the bottom-left texel of a render target `[IR:Guides/1_composite]`.
- Matrices are column-major as GLSL reads them; `gbufferModelView[3]` is the translation column `[IR:How To/coordinate_spaces]`.
- Colour encoding: `[SD:uniforms]` notes that `fogColor` and `skyColor` hold sRGB-encoded values. No source describes
  sRGB-encoded render targets; the spec team's reading is that stored values are plain numbers the pack controls (OQ-39).

### 5.6 Smoothing

`eyeBrightnessSmooth`, `centerDepthSmooth` and `wetness` follow their target with a half-life in ticks (the remaining
distance to the target halves every half-life), using the constants of 7.2. `[OF-SH]`, `[IR:Constants/*Halflife]`
How the smoothing is stepped (per frame with frame time, or per tick) is not documented: OQ-26.

---

## 6. Render targets

### 6.1 colortex buffers

#### 6.1.1 Count and aliases

- 16 buffers, `colortex0`–`colortex15`; at least 4 on any machine; E raises the limit to 32. `[OF-SH Color Attachments]`, `[IR:Buffers/colortex]`
- Aliases: `gcolor`=0, `gdepth`=1, `gnormal`=2, `composite`=3, `gaux1`–`gaux4` = 4–7. `[OF-SH Draw Buffer Index]`
- `[OF-SH Fragment Shader Configuration]` ties buffer creation to use: declaring `gaux1`–`4` / `colortex4`–`7` sets how
  many buffers exist (5–8). Buffers no program reads or writes need not exist.

#### 6.1.2 Size

- The render size (window size × render-quality factor, `viewWidth` × `viewHeight`) unless `size.buffer.<name>` sets
  another. `[IR:Buffers/colortex]`, `[OF-SP]`

#### 6.1.3 Formats

- Default `RGBA` = RGBA8 in practice. `[IR:Buffers/colortex]`
- `const int <buffer>Format = <FORMAT>;` sets a buffer's format, once per pack, in a block comment (1.7). The name may be
  `colortexN` or an alias (`gaux4Format`). `[OF-SH]`, `[IR:Constants/buffer_format]`
- If any program declares `gdepth` and buffer 1 has the default format, buffer 1 becomes RGBA32F. `[OF-SH]`, `[IR:Buffers/colortex]`
- Format list in 6.7.

#### 6.1.4 Writing

- A fragment shader's output list comes from its `RENDERTARGETS` or `DRAWBUFFERS` comment (several in one file: OQ-17); output index n goes to
  the n-th buffer of the list. Without either, the program writes buffers 0–7 in order. At most 8 outputs at once. `[OF-SH]`, `[IR:Constants/rendertargets]`
- Begin, prepare, gbuffers, deferred and composite programs can write any colortex buffer. `[IR:Buffers/colortex]`
- A listed buffer that the shader does not write receives undefined data. `[IR:Constants/rendertargets]`
- Composite-style programs write with blending off. Writing a buffer that the same program samples gives artefacts
  unless the flip (6.1.6) separates them. `[OF-SH]`
- `final` writes to the screen; its output 0 is the displayed colour.

#### 6.1.5 Clearing

- Every frame the buffers are cleared: colortex0 to the current fog colour (alpha 1),
  colortex1 to (1,1,1,1), every other colortex to (0,0,0,0). `[OF-SH]`, `[IR:Buffers/colortex]`, `[IR:Constants/buffer_clear_color]`
- `const bool <buffer>Clear = false;` disables the clear (the buffer keeps last frame's content);
  `const vec4 <buffer>ClearColor = vec4(r,g,b,a);` changes the clear value. `[OF-SH]` restricts both to "composite and
  deferred programs"; `[IR]` allows them in any file (OQ-17).
- Clear value conversion: buffers with fewer channels take the first components; integer buffers take the float's bit
  pattern as a 32-bit integer; 8/16-bit buffers clamp. `[IR:Constants/buffer_clear_color]`
- The exact moment of the clear relative to `begin` / `prepare`, and which of the two flip buffers it clears: OQ-8.

#### 6.1.6 Flipping (ping-pong)

Each colortex buffer has two textures, A and B, and a mapping of the names "main" and "alt" onto them.
`[OF-SH Color Attachments]`, `[IR:rendering]`

- Gbuffers and shadow programs read "main" and write "main" (so they must not read a buffer they write).
- Composite-style programs (`begin`, `shadowcomp`, `prepare`, `deferred`, `composite`) read "main" and write "alt".
- After a composite-style program, each buffer it wrote is flipped (main and alt swap), so the next program reads the new
  data. `flip.<program>.<buffer>=false` suppresses that flip; `=true` forces a flip of a buffer the program did not
  write, which lets a later program read the other copy. `[OF-SH]`, `[OF-SP]`
- `deferred_pre` and `composite_pre` are virtual programs that exist only to carry `flip.*` keys before the first
  deferred / composite program. `[OF-SH]`
- A program with only compute shaders causes no flip. `[IR:rendering]`
- `[IR:rendering]` words the default as "flipping enabled for all programs"; `[OF-SH]` flips the buffers the program writes.
  OQ-8 covers this and whether the A/B mapping carries over to the next frame (it matters for buffers that are not cleared).
- shadowcolor buffers also have two copies and flip the same way after `shadowcomp` programs. `[IR:Buffers/shadowcolor]`

#### 6.1.7 Mipmaps

`const bool <buffer>MipmapEnabled = true;` in the fragment shader of a `begin`, `prepare`, `deferred`, `composite` or
`final` program regenerates that buffer's mip chain right before that program. `[OF-SH]`, `[IR:Constants/colortex_mipmaps]`

### 6.2 depth buffers

| Sampler | Content | Source |
|---|---|---|
| `depthtex0` | everything drawn so far | `[OF-SH]` |
| `depthtex1` | without translucent geometry (water, stained glass) | `[OF-SH]` |
| `depthtex2` | without translucent geometry and without the hand | `[OF-SH]` |

- Window size, single channel, 24- or 32-bit, cleared to 1.0, never flipped, not configurable. Reading before the
  corresponding gbuffers have run returns undefined data. `[IR:Buffers/depthtex]`
- Gbuffers programs write depth automatically (`gl_FragCoord.z`), or through `gl_FragDepth`, which must then be written on
  every path. `[IR:Buffers/depthtex]`

### 6.3 Shadow map buffers

| Buffer | Content | Notes | Source |
|---|---|---|---|
| `shadowtex0` | depth of everything in the shadow pass | | `[OF-SH Shadow buffers usage]` |
| `shadowtex1` | depth without translucent geometry | | |
| `shadowcolor0`, `shadowcolor1` | colour written by the shadow and shadowcomp programs | cleared to (1,1,1,1) by default `[IR]`; `shadowcolor<N>Clear`, `ClearColor`, `Format` as for colortex; output index n of the shadow program writes shadowcolor n unless `RENDERTARGETS` says otherwise `[IR:Guides/4_shadows]`, `[IR:Buffers/shadowcolor]` | |

- Square, `shadowMapResolution` pixels (default 1024). `[OF-SH]`, `[IR:Constants/shadowMapResolution]` The host's shadow
  quality setting (`MC_SHADOW_QUALITY`, 1.5.1) presumably scales it; not stated (OQ-39).
- Depth buffers are cleared to 1.0, never flipped. `[IR:Buffers/shadowtex]`
- Declaring `shadow`/`shadowtex0` asks for one shadow depth buffer, `watershadow`/`shadowtex1` for two;
  `shadowcolor`/`shadowcolor0` for one colour buffer, `shadowcolor1` for two. `[OF-SH Fragment Shader Configuration]`
- Hardware filtering: `const bool shadowHardwareFiltering = true;` (or `...0`, `...1` per buffer) turns the shadow depth
  samplers into depth-comparison samplers (`sampler2DShadow`: sample with xyz, get the filtered comparison).
  E with `SEPARATE_HARDWARE_SAMPLERS`: comparison samplers are `shadowtex0HW`/`shadowtex1HW` and the plain names keep
  returning depth. `[OF-SH]`, `[IR:Buffers/shadowtex]`
- Filtering and mipmaps: `shadowtex0Nearest` / `shadowtexNearest` / `shadow0MinMagNearest`, `shadowtex1Nearest` /
  `shadow1MinMagNearest`, `shadowcolor0Nearest` / `shadowColor0Nearest` / `shadowColor0MinMagNearest`, and the same for
  colour 1, select nearest filtering; `generateShadowMipmap` (both depth buffers), `shadowtexMipmap` / `shadowtex0Mipmap`,
  `shadowtex1Mipmap`, `generateShadowColorMipmap`, `shadowcolor0Mipmap` / `shadowColor0Mipmap`, `shadowcolor1Mipmap` /
  `shadowColor1Mipmap` generate mip chains after the shadow pass. `[OF-SH]`, `[IR:Constants/shadow_mipmaps]`

### 6.4 Fixed textures

- `noisetex`: square RGB8 white-noise texture, `noiseTextureResolution` (default 256) on a side; replaced by
  `texture.noise=<path>`; read-only. `[OF-SH]`, `[OF-SP]`, `[IR:Buffers/noisetex]`
- Atlases: `gtexture`, `normals`, `specular` are bound in gbuffers-style programs only; other passes reach them through
  custom textures. The `_n` / `_s` atlases come from the resource pack (LabPBR or older layouts; decoding is the pack's
  business). `[IR:Buffers/atlases]`, `[IR:How To/pbr_standards]`, `[SL:LabPBR_Material_Standard]`

### 6.5 Custom textures, images and storage buffers

**`texture.<stage>.<sampler>=<path>`** (A) `[OF-SP]`, `[IR:Buffers/custom_textures]` replaces what an existing sampler
name returns during one stage:

| Stage | Programs |
|---|---|
| `gbuffers` | gbuffers and shadow |
| `deferred` | deferred |
| `composite` | composite (and `final`, per `[IR]`; `[OF-SP]` says composite and final) |
| `shadowcomp`, `prepare` | those (listed by `[IR]`) |
| E `setup`, `begin` | those |

Path forms:
1. relative to `shaders/`: an image in the pack (PNG; `[IR]` says the other loader also takes JPG);
2. `minecraft:<path>`: a texture from the resource packs / game assets, `_n`/`_s` suffixes for the PBR variants;
3. `minecraft:textures/atlas/<atlas>.png` (e.g. `blocks.png`) and `minecraft:dynamic/lightmap_1` (also
   `light_map_1`): live game textures;
4. raw data: `<path> <type> <internalFormat> <dims...> <pixelFormat> <pixelType>`, type ∈ `TEXTURE_1D TEXTURE_2D
   TEXTURE_3D TEXTURE_RECTANGLE`, formats from 6.7. Several textures of different types may share one sampler name;
   the shader's sampler type selects one. Suffixes `.0`–`.9` keep keys unique.

`customTexture.<name>=<path>` (E) creates a new sampler `<name>` with the same path forms, visible in every program.

**`.mcmeta`:** a file `<texture>.mcmeta` beside a pack texture, JSON `{"texture": {"blur": bool, "clamp": bool}}`: blur =
linear filtering (else nearest), clamp = clamp to edge (else repeat). Defaults: images nearest + repeat; raw textures
linear + clamp. Not applicable to resource-pack textures. `[OF-SP]`, `[IR:Buffers/custom_textures]`

**`image.<name>`** (E, `CUSTOM_IMAGES`): `<sampler> <pixelFormat> <internalFormat> <pixelType> <clearEachFrame>
<relative> <x> <y> [<z>]`; relative sizes are fractions of the screen, absolute sizes integers; a third size makes a 3D
image. Up to 16 images. Usable as `image2D/3D <name>` and as a sampler `<sampler>`. `[IR:Buffers/custom_images]`
`[PACK]` writes the format words in lower case and uses option names as sizes (substituted, 1.5).

**`bufferObject.<index>`** (E, `SSBO`): index 0–8; `<bytes>` fixed, or `<bytesPerCell> true <sx> <sy>` relative to the
screen (`int(viewWidth*sx) * int(viewHeight*sy) * bytes`), or `<bytes> <file>` initialised from a pack file (larger file =
error). Bound as `layout(std430, binding = <index>) buffer`. Content persists across frames. `[IR:Buffers/ssbo]`

### 6.6 Optional and per-feature limits

`[IR:Misc/macos]`: compute shaders, images and SSBOs need GL 4.2/4.3-class hardware; another loader limits macOS to 8
colour buffers. Horizon runs on wgpu and decides its own limits (OQ-31).

### 6.7 Formats

Names accepted for `<buffer>Format`, raw textures and images `[OF-SH Texture Formats]`, `[IR:Buffers/image_format]`:

- Normalised: `R8 RG8 RGB8 RGBA8 R16 RG16 RGB16 RGBA16`; E `RGBA4 RGB565 RGBA2`.
- Signed normalised: `R8_SNORM RG8_SNORM RGB8_SNORM RGBA8_SNORM R16_SNORM RG16_SNORM RGB16_SNORM RGBA16_SNORM`.
- Float: `R16F RG16F RGB16F RGBA16F R32F RG32F RGB32F RGBA32F`.
- Integer: `R8I RG8I RGB8I RGBA8I R16I ... RGBA32I`; unsigned `R8UI ... RGBA32UI`; E `RGB10_A2UI`.
- Mixed: `R3_G3_B2 RGB5_A1 RGB10_A2 R11F_G11F_B10F RGB9_E5`.
- Pixel formats (raw data): `RED RG RGB BGR RGBA BGRA` and the `_INTEGER` forms.
- Pixel types: `BYTE SHORT INT HALF_FLOAT FLOAT UNSIGNED_BYTE UNSIGNED_BYTE_3_3_2 UNSIGNED_BYTE_2_3_3_REV UNSIGNED_SHORT
  UNSIGNED_SHORT_5_6_5(_REV) UNSIGNED_SHORT_4_4_4_4(_REV) UNSIGNED_SHORT_5_5_5_1 UNSIGNED_SHORT_1_5_5_5_REV UNSIGNED_INT
  UNSIGNED_INT_8_8_8_8(_REV) UNSIGNED_INT_10_10_10_2 UNSIGNED_INT_2_10_10_10_REV`; E `UNSIGNED_INT_10F_11F_11F_REV`,
  `UNSIGNED_INT_5_9_9_9_REV`.
- `[IR]` marks which formats can be bound as images (the 3-channel ones cannot).
- Name matching is case-insensitive in practice (`[PACK]` writes `rgba16f`, `red_integer`).

---

## 7. Shadow map parameters

### 7.1 Geometry

- The shadow pass draws the world from the shadow light (`shadowLightPosition`): orthographic by default;
  `shadowMapFov` (degrees) switches to a perspective projection. `[OF-SH]`, `[IR:Constants/shadowMapFov]`, `[IR:Programs/shadow]`
- `shadowDistance` (default 160 blocks): half-size of the area the shadow map covers; on its own it scales the projection
  and does not cull. `[OF-SH]`, `[IR:Constants/shadowDistance]`
- `shadowDistanceRenderMul` (default −1): when > 0, geometry farther than `shadowDistance × mul` from the camera is not
  drawn in the shadow pass; it does not change the projection. `[OF-SH]`, `[IR]`
- `entityShadowDistanceMul` (E, default 1): multiplies the distance for entity shadows. `[IR]`
- `shadowIntervalSize` (default 2 blocks): the shadow camera moves in steps of this many blocks so shadows do not shimmer;
  0 disables snapping. `[OF-SH]`, `[IR:Constants/shadowIntervalSize]`
- `sunPathRotation` (degrees, default 0): tilts the sun and moon path sideways; changes `sunPosition`, `moonPosition`,
  `shadowLightPosition` and the shadow direction. `[OF-SH]`, `[IR:Constants/sunPathRotation]`
- `shadowNearPlane` (E, default 0.05) and `shadowFarPlane` (E, default 256): near and far distances from the shadow camera, blocks. `[IR]`
- `voxelDistance` (E, default 160): radius kept unculled by `shadow.culling=reversed`; `shadowDistance` cannot be lower. `[IR]`
- `shadowMapResolution`: 6.3.
- The exact matrices (where the shadow camera sits, which way its up vector points, how snapping applies): OQ-6.

### 7.2 Other constants (`const` in a stage file)

| Constant | Default | Meaning | Source |
|---|---|---|---|
| `wetnessHalflife` | 600 ticks | `wetness` rising | `[OF-SH]` |
| `drynessHalflife` | 200 ticks | `wetness` falling | `[OF-SH]` |
| `eyeBrightnessHalflife` | 10 ticks | `eyeBrightnessSmooth` | `[OF-SH]` |
| `centerDepthHalflife` | 1 tick | `centerDepthSmooth` | `[OF-SH]` |
| `ambientOcclusionLevel` | 1.0 | strength of vanilla AO in vertex colours, 0..1 (0 off). The documented curve: brightness `x` becomes `1 − level × (1 − x)` | `[OF-SH]`, `[IR]`, `[SD:const-directives]` |
| `superSamplingLevel` | 1 | documented, never honoured by any loader `[SD]` | Legacy |
| `noiseTextureResolution` | 256 | 6.4 | |
| `countInstances` | 1 | 1.7 | Legacy |

Booleans written `= true;` and floats with an `f` suffix (`2.0f`) both occur in the documents `[OF-SH]`; the scanner must
accept both.

---

## 8. Settings UI

### 8.1 Options

Options are found by scanning the pack's stage files (and dimension folders) `[OF-SP]`, `[IR:shader_settings]`:

- **Boolean, default on:** `#define NAME // comment`. **Boolean, default off:** the same line commented out:
  `//#define NAME // comment`. A boolean is recognised only if the same file also tests it with `#ifdef NAME` or
  `#ifndef NAME` (`[OF-SP]`; `[IR]`: "at least once"). `#if defined NAME` alone does not count.
- **Value option:** `#define NAME <default> // [v1 v2 v3] description`. The bracketed list (space-separated) gives the
  allowed values in display order; the default is added if missing. Only int options can be tested with `#if`.
- **Const option:** a recognised `const` (list in 8.5) followed by `// [v1 v2 ...]` becomes an option. A const without a
  list is hidden unless a profile or screen uses it.
- The text after the option (after the value list) is the tooltip. `[OF-SP]`: tooltip lines split at ". "; a line ending
  in "!" shows red.
- The same option may appear in several files; it is set everywhere at once. If the files give different defaults the
  option is ambiguous: shown disabled and not changeable. `[OF-SP]`, `[IR]`
- Options defined in `#include`d files: OQ-13.

### 8.2 Screens

- `screen=<items>` lays out the main settings page; `screen.<NAME>=<items>` a sub-page. Items: an option name; `[NAME]`
  a button to sub-page NAME; `<profile>` the profile selector; `<empty>` a blank cell; `*` every option not placed on any
  page. Sub-pages can nest and be linked from several places. `[OF-SP]`, `[IR:shader_settings]`
- Without `screen`, every option is listed. `[IR]`
- Columns: 2 by default; `[OF-SP]` switches to 3 or more when a page has more than 18 options; `screen.columns` and
  `screen.<NAME>.columns` set it. Long names are shortened to fit. `[OF-SP]`
- `sliders=<options>`: these multi-value options show as sliders (values in list order); others are buttons that cycle.
  One `sliders` line for all pages. `[OF-SP]`, `[IR]`
- Buttons: left click next value, right click previous, Shift+click default. `[OF-SP]`

### 8.3 Profiles

- `profile.<NAME>=<items>`: `OPTION:value` or `OPTION=value` sets a value; `OPTION` sets a boolean on, `!OPTION` off;
  `profile.<OTHER>` copies another profile; `!program.<name>` (optionally `world-1/<name>`) disables a program. `[OF-SP]`
- The current profile is the one whose settings all match the current values; if none matches, "Custom". `[OF-SP]`
- How the `<profile>` button orders and cycles profiles is not documented (OQ-32).

### 8.4 `.lang` files

`shaders/lang/<locale>.lang`, `key=value` lines `[OF-SP]`, `[IR:shader_settings]`:

| Key | Meaning |
|---|---|
| `option.<NAME>` | display name of an option |
| `option.<NAME>.comment` | tooltip |
| `value.<NAME>.<value>` | display text of one value |
| `prefix.<NAME>`, `suffix.<NAME>` | text around every value |
| `screen.<NAME>`, `screen.<NAME>.comment` | sub-page title and tooltip |
| `profile.<NAME>`, `profile.<NAME>.comment` | profile name and tooltip |
| `profile.comment` | tooltip of the profile button |

- `[PACK]`: all three packs ship `en_US.lang` (upper-case region) where the documents say `en_us.lang`; the strings use
  Minecraft `§` formatting codes (colours, reset). OQ-18.
- Values in `value.<NAME>.<value>` keys are written as in the source, e.g. `0.4f`. `[OF-SP]`

### 8.5 Recognised `const` options

`shadowMapResolution shadowDistance shadowDistanceRenderMul shadowIntervalSize generateShadowMipmap
generateShadowColorMipmap shadowHardwareFiltering shadowHardwareFiltering0 shadowHardwareFiltering1 shadowtex0Mipmap
shadowtexMipmap shadowtex1Mipmap shadowcolor0Mipmap shadowColor0Mipmap shadowcolor1Mipmap shadowColor1Mipmap
shadowtex0Nearest shadowtexNearest shadow0MinMagNearest shadowtex1Nearest shadow1MinMagNearest shadowcolor0Nearest
shadowColor0Nearest shadowColor0MinMagNearest shadowcolor1Nearest shadowColor1Nearest shadowColor1MinMagNearest
wetnessHalflife drynessHalflife eyeBrightnessHalflife centerDepthHalflife sunPathRotation ambientOcclusionLevel
superSamplingLevel noiseTextureResolution` `[OF-SP]`.

### 8.6 Player-side shader settings

The base format assumes a host settings page with: pack selection, the pack's options (above), and the host options the
macros of 1.5.1 report: FXAA, normal map, specular map, render quality, shadow quality, hand depth, old hand light, old
lighting `[OF-SH H]`. Where chosen values are stored is not documented (OQ-33).

---

## 9. Open questions

The documentation is silent or contradictory on these. The implementer decides each from first principles and tests
against the packs' observable behaviour, records the decision, and never resolves it by looking at another
implementation. Questions back to the spec team go through the README's process.

| # | Question | What the sources say |
|---|---|---|
| OQ-1 | `MC_VERSION` for 26.3 | The documented rule is major×10000 + minor×100 + patch (examples 1.21.0 → 12100). Applied to 26.3 it gives 260300. `[PACK]` contains a test `MC_VERSION <= 12111`, which only shows packs expect newer versions to compare above 1.21.11. Confirm by running packs. |
| OQ-2 | Identify as which loader? | Packs branch on `IS_IRIS`, `IRIS_VERSION`, `IRIS_FEATURE_*`. Defining them claims another product's identity and version; not defining them takes the packs' base paths. Product decision for Ludovic. |
| OQ-3 | Fallback of `gbuffers_clouds` | `[OF-SH]` none; `[IR]`, `[SL]` `gbuffers_textured`. |
| OQ-4 | World border and glowing entities | World border: `gbuffers_textured_lit` `[OF-SH]` vs `gbuffers_textured` `[IR]`. `gbuffers_entities_glowing`: documented by `[OF-SH]`, not recognised per `[IR]`. |
| OQ-5 | End of the fallback chain, missing `final` | What a "built-in" program draws for geometry with no program (vanilla look into colortex0? nothing?), and what reaches the screen without `final` (colortex0 presumably). |
| OQ-6 | Exact matrices | Composite quad vertex positions and projection; shadow model-view (camera placement, up vector, snapping, rotation by `sunPathRotation`) and projection (ortho extents from `shadowDistance`, near/far). Packs only need them consistent with the uniforms they receive, but their constants (e.g. distortion) are tuned to common values. |
| OQ-7 | Numeric values of `MC_RENDER_STAGE_*` | Not given; `[OF-SH]` lists them in draw order. Ascending integers in that order are a natural choice. |
| OQ-8 | Clear and flip timing | When the colortex clear happens relative to `begin`/`prepare`; which of A/B is cleared; whether the flip state resets each frame (matters for `Clear=false` history buffers); `[IR]` "flip enabled for all programs" vs `[OF-SH]` "flip what was written". |
| OQ-9 | ID and type corner cases | Unmapped `mc_Entity.x` (−1 vs 65535), `blockEntityId` range (index says signed 16-bit, page says 65535 for unmapped), `heldItemId` −1 vs index range 0–65535; `mc_Entity` declared as vec2/vec3/vec4 in different sources; `hideGUI` int vs bool. |
| OQ-10 | `cameraPosition` wrapping | `[IR]`: one loader wraps every 30000 blocks walked or after a teleport of over 1000 blocks; another every 1000024 blocks; `[SD]` mentions a (−1000, 1000) range. Pick one and keep `previousCameraPosition` consistent across the wrap. |
| OQ-11 | Biome constants | Numeric value of each `BIOME_*`, which names exist (current and legacy), and what plain numbers in `[PACK]` comparisons refer to. |
| OQ-12 | Dimension folders | Only `.vsh`/`.fsh` loaded from them (`[OF-SP]`) or all stages; "any `worldN` folder present → base folder unused" (`[IR]`) vs per-dimension override (`[OF-SP]`); `dimension.properties` vs `worldN` precedence. |
| OQ-13 | Options from included files | `[OF-SP]` says options are parsed from `.fsh`/`.vsh`; packs keep their options in included `.glsl` files (their `.lang` names options that exist nowhere else), and the "same file" rule for booleans then needs a definition. |
| OQ-14 | `#include` inside false conditionals | Read or skipped, and cycles. |
| OQ-15 | Error reporting | How compile errors and missing files are shown to the player, and how line numbers map back through includes. |
| OQ-16 | `.properties` details | Backslash continuation and other Java-properties escapes; duplicate keys; whether option macros are substituted inside values (observed in `[PACK]`); comments vs directives. |
| OQ-17 | Where configuration is read from | Fragment shaders only (`[SD]`), any file (`[IR]` most pages), specific programs (`[IR]` for `shadowNearPlane`, `voxelDistance`, `entityShadowDistanceMul`); `<buffer>Clear`/`ClearColor` restricted to composite and deferred (`[OF-SH]`) or not. |
| OQ-18 | Lang files | Locale file name case (`en_US` vs `en_us`), fallback when the player's locale is missing, `§` codes. |
| OQ-19 | Shadow content defaults | Defaults of `shadowTerrain`, `shadowTranslucent`, `shadowBlockEntities` (not stated by `[OF-SP]`). |
| OQ-20 | Precedence of pack vs player settings | `clouds`, `oldHandLight`, `oldLighting`: `[OF-SP]` gives the player's video setting priority, `[IR]` says the pack overrides. |
| OQ-21 | `oldHandLight` default | true `[IR]` vs false (with optional auto-detection) `[SD]`. |
| OQ-22 | `scale` offsets | Documented by `[OF-SP]`, `[IR]`; `[SD]` says offsets are not supported. |
| OQ-23 | Default alpha test and blend per program | Only the translucent default blend and a typical `alphaTestRef` of 0.1 are documented. |
| OQ-24 | Undocumented keys | `breaksAnisotropy` and other unknown keys: ignore with a log line? |
| OQ-25 | Expression language | Operator precedence, int/float/bool conversions, integer division, behaviour on unknown names or type errors. |
| OQ-26 | Smoothing | `smooth()` fade times in seconds (`[OF-SP]`) or ticks (`[IR]`); how half-life smoothing is stepped (per tick or per frame by elapsed time). |
| OQ-27 | Core-profile names | Provide `modelOffset` and `chunkOffset` both? Is the core-profile vertex layout (`vaUV1`, integer `vaUV2`) needed at all, given that every documented pack path also works in compatibility mode? |
| OQ-28 | Fixed-function fog state | `gl_Fog` / `gl_FogFragCoord` are compatibility built-ins old packs read; no pack document says what they hold. |
| OQ-29 | GLSL leniency and stages wgpu lacks | How far to accept non-conforming GLSL (`[SL:Compiler_Behavior_Notes]`); geometry and tessellation stages, which wgpu/naga does not offer; `GL_ARB_geometry_shader4`. |
| OQ-30 | `depthtex1` unit in gbuffers | `[OF-SH]` gives unit 11 in one table and 12 in another (irrelevant to a name-based binding, listed for completeness). |
| OQ-31 | Limits | Number of colortex buffers (16 or 32), simultaneous outputs (8), images (16), SSBO size; 3-channel formats that the GPU API cannot render to. |
| OQ-32 | Profile button order and "Custom" | Order of profiles, what clicking does when on "Custom". |
| OQ-33 | Persistence | Where the selected pack and option values are stored, and what happens when a pack update removes an option. |
| OQ-34 | Behaviours documented as broken | `separateEntityDraws` ("may not work"), `playerBodyVector` (equals look), shadowcolor mipmaps ("not generated"), `separateAo` needing the shadow pass: implement the documented intent or the observed behaviour? |
| OQ-35 | Draw order inside gbuffers | Fixed only around `deferred` (2.1); within a phase, follow vanilla 26.3. Where vanilla 26.3 draws something these lists do not name (new geometry since 1.21.11), which program and render stage it gets. |
| OQ-36 | `fogColor` for the colortex0 clear | Which moment's fog colour (before sky? after?) and whether alpha is 1. |
| OQ-37 | `frameTimeCounter` while paused | `[IR]`: does not pause; base documents silent. |
| OQ-38 | Meaning of `backFace.*` | `[OF-SP]`: `true` renders back faces (default false). `[IR]`: "enables or disables back-face culling", culling on by default, which reads as the opposite polarity. Packs set it expecting the `[OF-SP]` meaning, which is the base format. |
| OQ-39 | Inferred, not stated | Render targets hold plain (non-sRGB-converted) values; `MC_SHADOW_QUALITY` and `MC_RENDER_QUALITY` scale the shadow map and the render buffers. The spec team inferred these from macro names and pack practice; confirm by test. |

Errata noted while reading (resolved by the GL registry, not open): `[SD:uniforms]` gives `GL_LINEAR` as 9279; the
registry value is 0x2601 = 9729, as `[IR]` states. `[OF-SH]` gives `shadowAngle` as 0..1; `[SD]` and `[IR]` explain it
is 0..0.5.

---

## 10. Sources

All fetched on 2026-10-24. Documentation only; no implementation source was opened.

| Tag | Document | URL | Revision | Licence |
|---|---|---|---|---|
| `[OF-SH]` | OptiFine `shaders.txt` | https://github.com/sp614x/optifine/blob/master/OptiFineDoc/doc/shaders.txt | commit `07ec2ca62f81` (2026-01-20, "1.21.11 docs") | No licence file in the repository (documentation published by OptiFine's author; the repository holds docs and an issue tracker, no OptiFine code). Facts and names only, not copied text |
| `[OF-SP]` | OptiFine `shaders.properties` | https://github.com/sp614x/optifine/blob/master/OptiFineDoc/doc/shaders.properties | same commit | same |
| `[OF-PF]` | OptiFine `properties_files.txt` | https://github.com/sp614x/optifine/blob/master/OptiFineDoc/doc/properties_files.txt | same | same |
| `[OF-TX]` | OptiFine `texture.properties` | https://github.com/sp614x/optifine/blob/master/OptiFineDoc/doc/texture.properties | same | same |
| `[OD]` | OptiDocs "Shaders (developers)" pages | https://optifine.readthedocs.io/shaders_dev.html (source https://gitlab.com/whoatemybutter/optifinedocs) | rendering of OptiFine doc commit `83d482c` | CC0 (stated on the site). Used only to cross-check the uniform list |
| `[IR]` | "shaders.properties" documentation site, Reference, How To and Guide pages | https://shaders.properties/current/reference/overview/ (source repository https://github.com/IrisShaders/docs, `src/content/docs/current/**/*.mdx`) | commit `98f84cbe456e` | MIT (repository licence). Documentation pages only; the guide links to tutorial pack code, which was not opened |
| `[SD]` | ShaderDoc | https://github.com/IrisShaders/ShaderDoc | commit `ddf473726710` | LGPL-3.0 (repository licence). Pages read: `uniforms.md`, `const-directives.md`, `iris-features.md`, `vertex-format-extensions.md`, `passes/composite.md`, `unsupported-features.md`, `directives/properties.md`, `README.md` |
| `[SL]` | shaderLABS wiki: "Rendering Pipeline (OptiFine, ShadersMod)", "Shader Stages", "Compiler Behavior Notes", "LabPBR Material Standard", "Legacy Shaderpacks"; "Coordinate Spaces" cited through `[IR]` | https://shaderlabs.org/wiki/Main_Page | page text as of 2026-10-24 | CC BY-NC-SA 4.0 (site footer). Facts only |
| `[GLSL]` | The OpenGL Shading Language, Version 4.60.8 | https://registry.khronos.org/OpenGL/specs/gl/GLSLangSpec.4.60.html | 4.60.8 | © The Khronos Group, freely published specification |
| `[GLSL120]` | The OpenGL Shading Language, Version 1.20 | https://registry.khronos.org/OpenGL/specs/gl/GLSLangSpec.1.20.pdf | 1.20 (URL checked, not downloaded; cited for keyword facts only) | © The Khronos Group, freely published specification |
| `[GL]` | OpenGL 4.6 compatibility profile specification; `gl.xml` registry | https://registry.khronos.org/OpenGL/specs/gl/glspec46.compatibility.pdf ; https://github.com/KhronosGroup/OpenGL-Registry/blob/main/xml/gl.xml | gl.xml at commit `6af574a14089` | spec © Khronos; `gl.xml` Apache-2.0 |
| `[LOGL]` | LearnOpenGL, "Normal Mapping" | https://learnopengl.com/Advanced-Lighting/Normal-Mapping | — | text/images CC BY 4.0, code samples CC BY-NC 4.0 (site's About page). Technique only, no code |
| `[PACK]` | `shaders.properties`, `block.properties`, `item.properties`, `entity.properties`, `dimension.properties`, `lang/en_US.lang` and folder layout of BSL 10.1.8, Complementary Reimagined r5.9.3, Sildur's Vibrant 2.02 Extreme | local copies (downloaded from Modrinth 2026-10-15) | as named | each pack's own licence; redistribution forbidden. Only key names and syntax recorded here, no content |

Downloaded copies and their SHA-256 are in [provenance-log.md](provenance-log.md).

---

## 11. Provenance by section

| Section | Sources used |
|---|---|
| 1.1 Container | OF-SH, IR (overview, guide 1), PACK |
| 1.2 Dimension folders | OF-SP, IR (dimension_properties, ordering), PACK |
| 1.3 Program files | OF-SH, IR (programs overview, constants workGroups*) |
| 1.4 `#include` | OF-SP, IR (include, shaders.properties overview) |
| 1.5 Preprocessor, standard macros | GLSL §3.3, OF-SH, OF-SP, OF-TX, IR (macros/*, flags, overview), PACK |
| 1.6 `#version`, built-ins | GLSL §3.3, §7, §8.9.5; OF-SH; IR (compatibility_vs_core, attributes, patcher, guide 6); SL (compiler notes) |
| 1.7 Comment directives | OF-SH, IR (constants/*), SD (const-directives), SL (pipeline) |
| 2 Programs and order | OF-SH, OF-SP, IR (programs/*, gbuffers, shadow, ordering, depthtex, guide 1/2), SD (passes/composite), SL (pipeline) |
| 3 shaders.properties | OF-SP, OF-PF, IR (shaders.properties/*), SD (directives/properties), PACK |
| 4 ID mapping | OF-SH, OF-PF, IR (misc/*, macros/IRIS_TAG_SUPPORT, attributes/mc_Entity), PACK |
| 5 Interface | OF-SH, OF-SP, IR (uniforms/*, attributes/*, buffers/*, macros/*, how-to), SD (uniforms, vertex-format-extensions, unsupported-features), GLSL §7, GL (enums), LOGL, SL (pipeline, coordinate spaces via IR) |
| 6 Render targets | OF-SH, OF-SP, IR (buffers/*, constants/*, rendering), SD (const-directives), SL (pipeline, LabPBR), PACK |
| 7 Shadow parameters | OF-SH, IR (constants/*, programs/shadow), SD (const-directives) |
| 8 Settings UI | OF-SP, OF-SH (H), IR (shader_settings, overview), PACK |
| 9 Open questions | all of the above; GL for the errata |

---

## Changelog

- Revision 1 (2026-10-24): first version, from the sources of section 10.
