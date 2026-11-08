# Shader API

Horizon has its own shader packs: WGSL written against stages of Horizon's renderer, checked before it runs. This
page is the **Horizon shader API**. A separate clean-room loader reads the existing `shaders/` pack format (its own
spec, not part of this API); both sit on the same renderer hooks. The earlier implementation was removed on
2026-10-24; this API was written from scratch after it.

## Overview: what a shader author will be able to do

Write WGSL against documented stages of Horizon's renderer, with typed inputs, options the player can change, a
reload as soon as a file is saved and an overlay that shows what each pass costs and draws. The renderer keeps
everything that makes it fast (culling, batching, LOD, the mesher); a pack changes how surfaces and light look, not
how the world is fed to the GPU.

## The design

- **A pack** is a folder or a zip with `horizon-shaders.toml` (id, name, version, the `api` line, the stages it
  uses, its options) and WGSL files. No preprocessor and no other shading language: WGSL is what wgpu validates and
  compiles for Vulkan, Metal and DirectX 12.
- **Stages, not pipelines.** A stage is a function with a fixed signature that the renderer calls inside its own pass,
  for example a terrain material (`fn material(input: TerrainInput) -> Surface`), the lighting of the visible
  surfaces, the sky, an optional shadow pass, and a chain of full-screen post passes ending in the frame. A pack
  that leaves a stage out gets vanilla's look for it.
- **Typed inputs.** Each stage receives structs (the frame: time, camera, sun and moon, weather, fog; the surface:
  position, normal, block and its light) whose WGSL is generated from the Rust types the renderer uploads. The
  generated reference lists every field with its unit and range, and a test fails when it drifts.
- **Options are WGSL `override` constants.** The manifest declares each option (on/off, a number in a range, a choice)
  with its default; the game shows them on the pack's settings screen and sets them when it builds the pipeline. No
  text substitution, and every option combination is validated the same way.
- **Hot reload.** The game watches the pack's folder. A saved file is validated with naga and the stage rebuilt; on an
  error the last good version keeps drawing and the error, with its file and line, is shown in the overlay.
- **Debug overlay.** GPU time per stage and pass (timestamp queries), the targets each pass writes (show any of them
  full-screen), the pack's options and its compile errors.
- **Budgets.** A pack declares the targets and passes it needs; the renderer refuses more than its limits, and the
  benchmark ([benchmark-report.md](../benchmark-report.md)) records the pack and its cost, so "fast" stays measured.
- **Off by default.** Without a pack the renderer is vanilla's look, and no pack code runs.

## The pack format (schema 1)

A pack is a folder, or the same files zipped as a `.hshader`, with `horizon-shaders.toml` at its root
(crate `horizon-shader-api`; the sample with every key is
`crates/horizon-shader-api/tests/samples/schema-1/`):

```toml
schema = 1                 # the manifest's format; a reader refuses schemas newer than it knows
id = "soft_light"          # [a-z0-9_]{1,64}: the settings key and the registry id
name = "Soft Light"
version = "1.0.0"
api = "0.1"                # the shader API line the WGSL is written for
description = "..."        # optional, like authors and license
shared = ["common.wgsl"]   # optional: WGSL put before every stage's file, in order

[sky]                      # each stage is optional, but a pack has at least one
file = "sky.wgsl"
clouds = true              # optional: false when the pack draws its own clouds
sun = true                 # optional: false when the pack draws its own sun (the moon stays)
stars = true               # optional: false when the pack draws its own stars

[material]
file = "material.wgsl"
files = ["pbr.wgsl"]       # optional, on any stage: WGSL put after `shared`, before this stage's file
fog = false                # optional: the pack fogs its surfaces itself (Horizon does when absent or true)

[vertex]                   # needs [material]: where corners of terrain are drawn, for the world and the shadow map; shares the material's module
file = "vertex.wgsl"

[weather]                  # the colour of rain and snow streaks
file = "weather.wgsl"

[shadow]                   # optional, needs [material] or [[post]]: a shadow map from the sun (the moon by night)
resolution = 2048          # its side in texels, 256 to 8192
distance = 128.0           # half the side of the square it covers, in blocks, 32 to 512
solid_materials = [10]     # optional: [[materials]] ids drawn into the map whole, cutouts ignored
option = "shadows"         # optional: drawn while this option is not 0 (else always); off, there is no shadow pass
resolution_option = "shadow_resolution"   # optional: an int, or a choice of numbers, whose value replaces resolution
distance_option = "shadow_distance"       # optional: the same for distance

[[post]]                   # full-screen passes, in the order they run (8 at most)
file = "post/grade.wgsl"
name = "Colour grade"      # optional: the GPU time overlay's label
output = "bloom"           # optional, not on the last pass: writes this target, read by name in later passes
scale = 0.5                # optional, with output: 1, 0.5 or 0.25 of the world's size

[[options]]                # 64 at most; `id` is the WGSL `override` it sets
id = "saturation"
name = "Saturation"
type = "float"             # bool, float (min, max, step), int (min, max), choice (values)
default = 1.1
min = 0.0
max = 2.0
step = 0.05

[[materials]]              # 1024 at most: MaterialInput.material, the first entry that matches
id = 10                    # 1 to 65535; 0 is a sprite no entry names
sprites = ["block/*_leaves"]   # sprite names, any number of * each
blocks = ["oak_log", "grass_block:snowy=true"]   # optional: the sprites these blocks' models draw with, where no entry's sprites name them
translucent = false        # optional: this id's sprites draw in the translucent pass, clear texels and all

[[pages]]                  # optional: the options page's layout, the first the one it opens on
id = "main"
name = "Sample"
items = ["page:colour", ""]   # a button opening a page, an empty slot (or an option's id)

[[pages]]
id = "colour"
name = "Colour"            # the page's title and its button's label
description = "How colourful the picture is."   # optional: the button's tooltip
columns = 1                # optional: 1 or 2 (default) buttons a row
items = ["saturation"]

[[textures]]               # 8 at most: a PNG every stage reads as noise and noise_sampler
id = "noise"
file = "noise.png"
filter = "linear"          # optional: linear (default) or nearest
wrap = "repeat"            # optional: repeat (default) or clamp

[[lights]]                 # 256 at most: block_light_color's colour, the first entry that matches
color = [0.3, 0.7, 1.0]    # each channel 0 to 4
blocks = ["soul_*"]        # block names, any number of * each

[[particle_lights]]        # 32 at most: these particles join the frame's coloured lights
particles = ["firefly"]    # names, `firefly` or `minecraft:firefly`; the first entry naming one wins
color = [0.8, 0.9, 0.4]    # each channel 0 to 1; the particle's alpha is its intensity
reach = 4.0                # blocks, 0.25 to 16; the 16 nearest lights shine, read by MaterialInput.colored_light

[[tags]]                   # 32 at most: bit i of MaterialInput.tags is the i-th tag
id = "glass"
sprites = ["block/glass", "block/*_stained_glass"]   # sprite names, at most one * each

[budget]
gpu_ms = 1.5               # optional: the GPU time the pack aims to add (0 to 16 ms)

[reflection]               # optional, needs [material]: when the water mirror (`reflection`) is drawn
option = "reflections"     # optional: drawn while this option is not 0 (else whenever the material reads it)

[displacement]             # optional, needs [material]: relief as geometry near the camera
distance = 12.0            # optional: how far, in blocks (4 to 32); eases out over the last quarter
cells = 16                 # optional: grid cells along a face's side (4 to 64)
option = "displacement_on" # optional: a bool option that turns it on (else always on)
cells_option = "cells"     # optional: a choice option of cell counts that replaces `cells`
steps_option = "steps"     # optional: a bool option: each cell a flat step with walls while it's on
```

Under `[displacement]` the faces of solid terrain whose textures have LabPBR heights (the normal map's alpha) are
cut into a grid near the camera, and each corner moves along the face's normal by the material stage's
`fn displacement(input: DisplacementInput) -> f32`: relief that is geometry, which hides itself, writes its
depth and is lit where it is. `MaterialInput.displaced` says how much of the relief a point already got that way,
so a parallax effect fades out where the geometry took over.

An option's `override` has the type of its kind: `bool`, `f32`, `i32`, or `u32` for a choice (the index of the
value). The pack holds only the manifest, the `.wgsl` files it names (each used, UTF-8, 256 KiB at most),
`README.md`, `LICENSE` (`.md`, `.txt`) and `icon.png`, under plain paths, 8 MiB in all. A `.hshader` is built
the same way every time (files in path order, fixed dates), so a pack's SHA-256 depends only on its files.

Native selections are saved as `horizon:<id>`; option values live in
`shaderpacks/.horizon-options/<id>.txt`. Updates and file renames keep both. Existing filename
selections and sidecars are imported without deleting the originals; after an update has removed
the old file, migration recognizes the launcher's `<id>-<version>.hshader` names. An arbitrary
removed filename cannot be mapped back to an ID. If several installed packs share an ID, the first
filename in lexical order is used and listed. The `HORIZON_SHADER_PACK` development override still
names an exact filesystem path; `shaders/` packs still use filenames.

## Shader API 0.2 (experimental)

0.2 is the line for linear light and HDR ([hdr-linear-light.md](../design/hdr-linear-light.md)). A pack opts in with
`api = "0.2"`. `horizon-shader check` and `pack` take it; **the game draws 0.2 packs only with
`HORIZON_HDR_FRAME=float` set, and the registry doesn't list them**. With the switch, a 0.2 pack draws the world
into a float frame where the device can (else the 8-bit one): the frame follows the picked pack,
rebuilding the renderer and remeshing the world when needed. A refused or missing pack, or one whose owner mod is off,
uses the no-pack frame. What it changes from 0.1:

- **Linear light.** Every colour a stage reads or returns is linear light, 1.0 at paper white and more brighter. Horizon
  decodes vanilla's values with a 2.2 power and encodes the results the frame stores. Depth, normals, light levels,
  `emission`, the LabPBR maps and `[[textures]]` stay data. `[[lights]]` and `[[particle_lights]]` colours are still
  written in vanilla's gamma, and Horizon decodes them. Alpha is coverage only, and a post pass's alpha is ignored.
- **The display in `Frame`:** `hdr`, `headroom`, `paper_white`, `peak` and `black_level`, after `shadow_center_texel`.
  Until HDR output is served for 0.2 packs (a later step), a 0.2 pack sees SDR values with HDR on too (`hdr` 0,
  `headroom` 1, `paper_white` and `peak` 80, `black_level` 0), and its picture is shown at paper white with nothing
  lifted.
- **`[tonemap]`**, `fn tonemap(input: TonemapInput) -> vec3<f32>`: the post chain's picture in, the picture for the
  display out, from 0 to `frame.headroom`, once per pixel of the window. Without it a pack gets Horizon's curve.
- **`[frame] blend`**: `"gamma"` (the default) or `"linear"`, what the frame stores and so how see-through draws
  blend. `"linear"` passes the check, but the game doesn't serve it yet.

```toml
api = "0.2"

[tonemap]                  # optional: the pack's own tone curve
file = "tonemap.wgsl"

[frame]
blend = "gamma"            # optional: gamma (default) or linear
```

0.1 is unchanged: the same prelude, entries and reference, and a 0.1 pack that names `[tonemap]` or `[frame]` is
refused. The example's port is `crates/horizon-shader-api/packs/golden_hour_0_2`, a test pack of the SDK.

## Testing a pack frame by frame

`horizon-ctl screenshot-sequence FRAMES STEP_MS PATH`, under `HORIZON_STILL`, starts the running pack over and draws
its first FRAMES frames on a set clock: frame n has `frame_index` n, `time` (n - 1) × STEP_MS / 1000 and `delta_time`
STEP_MS / 1000 (0 on frame 1; STEP_MS may be a decimal, `16.666667` for 60 fps), and `history` and the eased values
start as in a pack just loaded. The last frame is written as a PNG, the same bytes every run on the same build, GPU
and driver. Packs in the `shaders/` format get the same (`frameCounter`, `frameTimeCounter`, `frameTime`). How to use
it: [the guide](../shaders.md#testing-a-pack-frame-by-frame).

## Guide, reference and examples

- [shaders-reference.md](shaders-reference.md): every stage's signature, input and output structs, resources and
  helpers, the frame's fields with their units, option types and limits, generated from `crates/horizon-shader-api`.
  0.2's is [shaders-reference-0.2.md](shaders-reference-0.2.md).
- [The guide](../shaders.md): making a pack with `horizon-shader new`, `check` and `pack`; the templates and the
  example pack (`crates/horizon-shader-api/packs/golden_hour`).

## Stability

Experimental (API 0.1). The stages ship in the game behind pack selection; see [stability.md](stability.md). API
0.2 is experimental too, and the game draws it only with `HORIZON_HDR_FRAME=float`.
While the API is experimental, a line is served until the next one is on by default: 0.1 ends when 0.2 is
([hdr-linear-light.md](../design/hdr-linear-light.md), step 5).

## Not there yet

An overlay showing what each pass draws (F3 already shows what each costs), and stages for entities, block entities
and particles (the material stage covers terrain only).

## Changelog

- 2026-11-08: API 0.2 (change, experimental): `horizon-shader check` refuses a 0.2 pack that constructs `Frame`, a
  stage input (`PostInput(...)`) or `SurfaceData`, which may gain fields; reading and passing them along are
  unchanged, and `Surface` is still constructed. The reference says `[tonemap]` runs once per pixel per output. API
  0.1 is unchanged.
- 2026-11-08: API 0.2 (fix, experimental): the world a post pass reads holds coverage in its alpha, as the
  reference says, instead of Horizon's light-source marks. API 0.1 is unchanged.
- 2026-11-07: API 0.2 (change, experimental): with `HORIZON_HDR_FRAME=float`, a 0.2 pack picked or dropped gets its
  frame at once instead of at the next start. API 0.1 is unchanged.
- 2026-11-05: API 0.2 (change, experimental): the game draws 0.2 packs with `HORIZON_HDR_FRAME=float` set, on a
  float frame where the device can; `[frame] blend = "linear"` is still refused. API 0.1 is unchanged.
- 2026-11-05: API 0.2 (new line, experimental): linear light, `Frame`'s display fields, `[tonemap]` and
  `[frame] blend`, with its own reference. `horizon-shader` checks and packs it; the game and the registry refuse it
  for now. API 0.1 is unchanged: the uploaded `Frame` buffer grows by 32 bytes after the fields 0.1 declares.
- 2026-11-04: API 0.1 (change, experimental): the post stage's `scene_surface` is `texture_2d<u32>`, the bits of
  the stored floats (RGBA32F is not renderable on OpenGL). Read it through `surface_at`, or
  `bitcast<vec4<f32>>(textureLoad(scene_surface, p, 0))`; a `textureLoad` as `vec4<f32>` no longer validates.
- 2026-11-03: API 0.1 (no change to packs): float and int `[[options]]` are sliders on the options page, on their
  `step`; the value is saved, and the pack rebuilt, when the slider is let go.
- 2026-11-03: API 0.1 (additions): `[shadow] option`, `resolution_option` and `distance_option`: a pack's
  options can turn its shadow map off (no map, no shadow pass, `has_shadow` 0) and pick its size, so
  one pack offers its own quality levels. The options sizing the map need no `override`.
- 2026-11-03: API 0.1 (additions): with `[shadow]`, `[displacement]`'s grid casts into the shadow map in
  the flat faces' place, so the relief shadows itself.
- 2026-11-03: API 0.1 (additions): `[displacement]` draws: near the camera the faces with LabPBR relief are a
  grid moved by `displacement`, and `MaterialInput.displaced` says how much of it a point has.
- 2026-11-03: API 0.1 (additions): the manifest's `[displacement]`, the material stage's
  `fn displacement(input: DisplacementInput) -> f32` and `MaterialInput.displaced`.
- 2026-11-02: API 0.1 (additions): with dynamic lights on, held and dropped items light
  `MaterialInput.colored_light` in their block's `[[lights]]` colour.
- 2026-11-02: API 0.1 (additions): `[[particle_lights]]` and `MaterialInput.colored_light`.
- 2026-11-01: API 0.1 (additions): `[[materials]]` and `MaterialInput.material`, an id by sprite,
  and `[shadow] solid_materials`.
- 2026-11-01: API 0.1 (additions): entities cast into `shadow_map`, the player's own model in first person too, and the round
  blob under entities is left out in the Overworld while a pack draws a shadow map, so under a
  roof they have no shadow but what the pack gives them. Only entities the camera's culling keeps
  cast: one off screen or behind a wall leaves the map, and its shadow with it.
- 2026-11-01: API 0.1 (additions): `[sky] clouds`, `sun` and `stars`: a sky can replace Minecraft's.
- 2026-11-01: API 0.1 (additions): `[[pages]]`, the layout of a pack's options page.
- 2026-11-01: API 0.1 (additions): the `[weather]` stage
  (`fn weather(input: WeatherInput) -> vec4<f32>`) colours rain and snow streaks.
- 2026-11-01: API 0.1 (additions): `[[textures]]`, PNGs bound for every stage.
- 2026-11-01: API 0.1 (additions): `[[lights]]` and the material stage's `block_light_color`.
- 2026-10-31: API 0.1 (additions): translucent terrain in the shadow pass: `shadow_map_translucent`
  and `shadow_color`, so a pack can tint the light that passes through water and stained glass.
- 2026-10-31: API 0.1 (additions): `Frame.mods`, eight `vec4<f32>` a native mod hands the pack it bundles each
  frame (`native_shader_params` in docs/native-mods.md), all 0 when no mod sets them.
- 2026-10-31: API 0.1 (additions): `[shadow]`: the engine draws the solid terrain the light sees
  into a distorted depth map each frame, which the material and post stages read as `shadow_map`
  with `shadow_sampler` and the prelude's `shadow_position`; the frame's `light_direction`,
  `has_shadow`, `shadow_view_proj`, `shadow_distance`, `shadow_distortion`, `shadow_resolution`
  and `shadow_center_texel`.
- 2026-10-30: API 0.1 (additions): the manifest's `[reflection] option`, which draws the water
  mirror only while that option is on, and the mirror's own eye in facing culling.
- 2026-10-30: API 0.1 (additions): the material stage's `reflection` and
  `reflection_sampler`, the world mirrored about the water plane around the camera, drawn
  only for a pack whose material reads it; `Frame.water_plane`, `has_reflection` and
  `reflection_resolution`.
- 2026-10-30: API 0.1 (additions): `MaterialInput.face`, `fluid`, `emission`, `normal` and
  `tangent`: which fluid and face a point is on, how strongly its block shines, and the
  surface's frame for normal maps and lighting.
- 2026-10-30: API 0.1 (additions): the post stage's `history`, last frame's input to the last
  pass, so a temporal pass second to last reads its own previous output (TAA, denoising).
- 2026-10-30: API 0.1 (additions): `Frame.previous_view_projection`, `camera_motion` and
  `delta_time` for temporal passes (reprojection, motion blur); `eye_light_smooth`, `in_water`
  and `in_lava`, eased so exposure and underwater fog fade instead of stepping.
- 2026-10-30: sprite tags (`[[tags]]`, `MaterialInput.tags`) and the frame's `precipitation`, `temperature`,
  `downfall` and `wetness`.
- 2026-10-30: API 0.1 (additions): the material stage's `MaterialInput.screen` and its
  `opaque_depth` and `opaque_color`, the world as it was before translucent terrain drew, so
  water and glass can see what lies behind them (depth under water, refraction).
- 2026-10-26: the material stage in the game: terrain's surfaces through the pack's `material`.
- 2026-10-26: the sky stage in the game: the pack's `sky` gets vanilla's colour along each ray.
- 2026-10-26: the post stage in the game (`HORIZON_SHADER_PACK`, hot reload with the last good pack kept).
- 2026-10-25: `horizon-shader` (`new`, `check`, `pack`), three templates, the example pack and the guide.
- 2026-10-25: the check (naga validation, signatures, options, budgets, SPIR-V/Metal/HLSL translation).
- 2026-10-25: the stages of API 0.1 (sky, material, post), their inputs and the generated reference.
- 2026-10-25: the pack format, schema 1: `horizon-shaders.toml` and the `.hshader` package.
- 2026-10-24: the earlier shader pack code was removed; this API is designed from scratch.
- 2026-11-01: API 0.1 (additions): a stage's `files`.
- 2026-11-01: API 0.1 (additions): `[material] fog`.
- 2026-11-01: API 0.1 (additions): the material stage reads the resource packs' LabPBR maps (`_n`, `_s`): `block_pbr_maps`, `block_normal_map`, `block_specular_map`, their `_level` forms and `block_pbr_size`. The maps load only while a native pack is picked: without one they cost nothing, and the first pick reloads the resources once.
- 2026-11-01: API 0.1 (additions): `[[post]] output` and `scale`, targets of a pack's own at the world's size, half or a quarter of it.
- 2026-11-01: API 0.1 (additions): the material stage's `block_texture`, `block_texture_level`, `block_texture_size` and `block_texture_lod`, the surface's own sprite anywhere on it. The LabPBR `_level` functions now count the block texture's mip levels, the same footprint where a map is smaller.
- 2026-11-01: API 0.1 (additions): the material stage's `set_surface` and the post stage's `surface_at` and `scene_surface`: a normal, a smoothness and a value of the pack's own per pixel of solid terrain. Only a material stage that calls `set_surface` pays for the target.
- 2026-11-01: API 0.1 (additions): `[[materials]] blocks`, ids by block, and `translucent`, moving an id's sprites into the translucent pass. `sprites` may be left out when `blocks` is there.
- 2026-11-01: API 0.1 (additions): the `[vertex]` stage, `fn vertex(input: VertexInput) -> vec3<f32>`: waving plants and rolling water, in the world and the shadow map.
