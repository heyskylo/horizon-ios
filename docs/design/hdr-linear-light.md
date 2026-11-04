# Linear light for HDR and shader packs (shader API 0.2)

A design note, before any code: what a colour means to a shader pack from API 0.2 on, how the frame stores the
picture, and how HDR output stops reading light from the frame's alpha. Part of [#3125](https://github.com/horizon-rust/horizon/issues/3125)
and [#1441](https://github.com/horizon-rust/horizon/issues/1441). The shader API is experimental (0.1), which needs a
design note for a new line ([api/stability.md](../api/stability.md)); this is that note.

## Why

**The frame can't hold light brighter than white.** The world is drawn in vanilla's gamma space into the swapchain's
8-bit format, or with HDR on into `Rgba16Unorm`, which clips at white after every pass as the 8-bit frame does
(`gfx.rs` `WIDE_FRAME`). HDR output (`hdr.wgsl`) rebuilds the light above white at the end, from marks in the frame's
alpha:

| Alpha | Means |
| --- | --- |
| below 0.75 | a light source: its tier and how much it is lifted |
| 0.76 to 1 | HDR lighting's gain on a surface |
| 1/255 | the first-person hand |
| what the HUD and menus leave | how much they cover the world |

To tell the world from the HUD, `Hdr::keep_world` copies the whole frame every frame and `shows_world` compares
the two within a tolerance. Anything that filters or blends alpha can break a mark: FXAA and TAA return the centre
pixel's alpha with smoothed colour (`world_post.wgsl` 54 and 132), and the resolution-scale copy (`fs_copy`) samples
alpha bilinearly, so a pixel between a torch and a wall can land in another tier's band. (Found in the code, not yet
measured on screen.)

**Packs can't make HDR.** A pack's last post pass draws onto that clipped frame, HDR lighting turns off while a pack's
pack runs, and post passes must return `input.color.a` so the marks survive. A pack's HDR would only
be possible by guessing highlights back from an SDR picture (inverse tone mapping).

**The API doesn't say what a colour is.** The reference gives colours as "0..1" and never says gamma or linear, or
whether more than 1 is allowed. The example pack reads them as gamma, capped at 1: Golden Hour's glow picks pixels by
`smoothstep(0.8, 1.0, luminance(c))` and its grade clamps to 0..1. Every pack written that way makes the meaning
harder to change.

**Vanilla is going the other way.** Mojang plans Vibrant Visuals for Java Edition (the render thread and the move to
Vulkan are its groundwork). On Bedrock it lights in HDR, in physical units, with auto exposure, bloom and a filmic
tone curve down to SDR at the end. A pack API capped at gamma 0..1 would have to break again for it.

## Goals

1. **HDR off and no pack: the picture is today's, bit for bit**, pinned by a screenshot test (below). Players without
   HDR or a pack run the code they run today.
2. **0.1 packs keep working**: unchanged in SDR, through a documented shim (below).
3. **Packs write light above white, on SDR and HDR alike.** SDR is HDR with no headroom: one pack, one tone curve,
   both displays.
4. **No light in alpha.** The marks, `keep_world` and `shows_world` go; alpha is coverage only.

Not goals: changing vanilla's look; physically based lighting without a pack; HDR for `shaders/`-format packs (see
below); where HDR's settings live (a later note).

## The colour rule (shader API 0.2)

- **Every colour a 0.2 stage reads or returns is linear light**: BT.709 (sRGB) primaries, scene-referred, **1.0 is
  vanilla's white at paper white**, more is brighter, up to half float's range. Horizon replaces negative values and
  NaN with 0 before the display.
- **Decoding is vanilla's 2.2 power**, as `hdr.wgsl` decodes today, not the piecewise sRGB curve. Because a power is
  multiplicative, vanilla's lit colour `albedo * tint * lightmap` is the same in linear: decoding the product is the
  product of the decoded terms.
- **What converts:** `MaterialInput` `albedo`, `tint`, `lightmap` and `colored_light`; `SkyInput.color`;
  `WeatherInput.color`; `Frame` `sky_color` and `fog_color`; the block texture and LabPBR helpers; every texture a
  stage samples (`scene_color`, `previous`, `history`, pack targets, `opaque_color`, `reflection`, `shadow_color`,
  `block_light_colors`). Data stays data: depth, normals, `light` levels, `emission`, LabPBR values.
- **Alpha is coverage only.** The rule to return `input.color.a` goes; a 0.2 post pass's alpha is ignored.
- **Display info in `Frame`**, appended after `shadow_center_texel`:

| Field | Type | Meaning |
| --- | --- | --- |
| `hdr` | `u32` | 1 while the output is HDR, else 0. |
| `headroom` | `f32` | The peak over paper white: the brightest value the display shows. 1 in SDR. |
| `paper_white` | `f32` | Paper white in nits; 0 in SDR (unknown). |
| `peak` | `f32` | The peak brightness in nits; 0 in SDR. |
| `black_level` | `f32` | The display's black in nits (HDR's Black Level); 0 in SDR. |

- **A `[tonemap]` stage**, `fn tonemap(input: TonemapInput) -> vec3<f32>`: the post chain's linear picture in, the
  picture for the display out, linear, 1.0 at paper white and at most `frame.headroom`. It runs once per pixel at
  the window's size, after the post chain, and owns the look of highlights. A pack that leaves it out gets Horizon's:
  in HDR a roll-off into the headroom (ITU-R BT.2390's EETF in PQ; HDR's black level already uses the same
  recommendation's lift), in SDR a clip at white, as vanilla. A pack that only grades keeps Horizon's tone curve; one that only tone maps needs no post chain.
- **Horizon's display pass is always last** and can't be replaced: it clamps to the headroom, converts to BT.2020
  for HDR10, lifts the black level, encodes (PQ, scRGB, or 2.2 for SDR) and composites the HUD.

## How the frame stores the picture

The frame depends on what draws the picture, not on the display:

| Picture | Frame | Stored as |
| --- | --- | --- |
| HDR off, no pack | the swapchain's 8-bit format, as today | vanilla's gamma, clipped |
| HDR on, no pack | `Rgba16Float` | vanilla's gamma, values above 1 kept |
| a 0.2 pack (SDR or HDR) | `Rgba16Float` | vanilla's gamma, values above 1 kept; linear if the pack asks (`[frame] blend = "linear"`) |
| a 0.1 pack | as today | as today (the shim) |

**Why gamma storage.** Vanilla blends glass, water, clouds, particles and weather by fixed-function blending in
gamma space. Keeping that encoding keeps every blend of values up to 1 as vanilla's, and solid surfaces and lighting
are the same in either encoding. With HDR on and HDR Lighting off the picture is SDR's except where vanilla would have
clipped at white before a later blend: the sun and moon add onto the sky (`RenderPipelines.CELESTIAL`, src alpha and
one), so clouds or glass over a bright sun come out a little brighter than vanilla's. Goal 1 covers HDR off only;
this case is held to the existing HDR-versus-SDR check (below).

A pack that wants blending in linear light, as a physically based pack does, asks for it; its translucent surfaces
then blend differently from vanilla's, which is the pack's choice.

**Converting at a stage's edges.** Horizon decodes what a 0.2 stage reads and encodes what it returns before the
frame stores and blends it. Horizon's own fog still mixes in gamma, as vanilla's does. The world's picture is decoded
once before a 0.2 pack's post chain (a full-screen pass, about what `keep_world` costs today), and in the copies
Horizon makes anyway (`opaque_color`). The water mirror and `shadow_color` draw no blends, so they store linear.

**Cost and support.**

- `Rgba16Float` is 8 bytes a pixel, as `Rgba16Unorm` is, so HDR on costs the same memory as today, and the per-frame
  `keep_world` copy goes.
- A 0.2 pack in SDR moves the frame from 4 to 8 bytes a pixel (about 15 MB more at 1440p), only while it runs. Its
  post chain already uses `Rgba16Float`.
- WebGPU renders to, blends and filters `Rgba16Float` without optional features, unlike `Rgba16Unorm`
  (`draws_wide_frame`), so the 8-bit HDR fallback goes. The GL backend is checked in the series.
- Up to 1, half float steps at least 8 times finer than 8-bit, and far finer in the darks, so Smooth Gradients'
  gain stays without its setting.

## Light above white without marks

Each shader that marks the frame today writes the light instead:

- **Light sources** (`mesher` `EMISSION_BITS`, `hdr_tier`, `gfx/hdr.rs` `TIERS`): the face's colour times its tier's
  gain, in units of paper white. Easing into the peak moves to the tone curve.
- **HDR lighting** (light levels 11 to 15, up to 4 times): the lightmap going past 1, which is what its alpha means
  now.
- **Where:** terrain and fluids (`terrain.wgsl`, `mesher/fluid.rs`, distant terrain in `lod/mesh.rs`, the visibility
  buffer), particles (`particles.rs`), the sun and moon (`fs_sky_celestial`), entities, dropped and held items
  (`entity.wgsl`, `dynamic_light.rs`), lightning (`gfx/lightning.rs`), and `native_material.rs` and `pack_atlas.rs`
  for packs.
- **The hand** draws at gain 1, as it stays at the world's brightness today.
- **The HUD and menus** draw into their own 8-bit target while HDR is on, and the display pass composites them at
  UI Brightness over the world's picture, as frame generation already keeps the world without the hand and HUD.
  With HDR off they draw onto the frame as today.

The target look for HDR on without a pack is today's, within the offscreen checks' tolerance, at Auto (203 and 1000
nits). Any change of look is a separate pull request, with screenshots.

Under a 0.2 pack Horizon lifts nothing: the pack gets vanilla's picture in linear light whatever the display, reads
`emission` in its material stage, and writes what shines. (Telling post stages which pixels are light sources is a
likely 0.2 addition, not designed here.)

## 0.1 packs: the shim

Horizon serves both lines (`manifest.rs` `API` becomes a list) and runs a 0.1 pack as it runs today:

- **In SDR**: today's frame, inputs and post chain onto the frame, bit for bit. Golden Hour stays at 0.1 as the
  shim's test pack; a 0.2 port sits beside it.
- **In HDR**: its stages read today's gamma values, clipped to 0..1, and their results are read as gamma. Light
  sources shine as they do today over a 0.1 pack: Horizon applies the light sources' gain after a 0.1 material, as the
  wrapper applies the mark today, and keeps the light above white past a 0.1 post chain by adding back what the chain
  was given above 1, as the lift on top of the pack's picture does today. HDR lighting stays off.
- **How long**: 0.1 is served at least until 0.2 leaves experimental; its removal goes in the changelog.

## `shaders/`-format packs

They keep their own buffers and tone map to SDR in their last program, and their alpha is theirs. With HDR on their
picture is shown at paper white with nothing lifted, as #3125 suggests. HDR values they could read (`hdr`, headroom,
paper white) are a later, optional addition for pack authors.

## Upscaling, anti-aliasing, frame generation

They read the frame as it is stored and have no marks to keep, which fixes the alpha problems above. Native upscaling
([#1403](https://github.com/horizon-rust/horizon/issues/1403)) should take the float frame. TAA over values above 1
may need a tone-weighted resolve so bright pixels don't sparkle. Frame generation stays off with HDR output, as today.

## Tests

1. **HDR off, no pack, bit for bit.** The frame format and pipelines for this case don't change (unit tests on
   `frame_format` and the pipelines' targets). A screenshot test pins the picture: pictures of the game hold
   Minecraft's textures and can't be committed, so it is differential. A runner with a copy of Minecraft and Mesa's
   software Vulkan builds main and the change, captures fixed scenes offscreen with `HORIZON_STILL=1` (repeatable
   frames, [#9398](https://github.com/horizon-rust/horizon/pull/9398), in review), and requires identical PNGs.
2. **Golden Hour (0.1) in SDR**: the same capture.
3. **HDR on, HDR Lighting off, against SDR**: the offscreen HDR capture ([#8947](https://github.com/horizon-rust/horizon/pull/8947)),
   within one 8-bit step outside the spots vanilla clips.
4. **HDR look against today**: the same captures within a tolerance, with screenshots in the pull request.
5. **Cost**: GPU time and memory, HDR on, against today at 1080p and 1440p, on a desktop GPU and an integrated one.

## Steps

Behind `HORIZON_HDR_FRAME=float` until the last step, each a small pull request:

1. This note.
2. Shader API 0.2 in `horizon-shader-api`: the rule, the `Frame` fields, `[tonemap]`, `[frame] blend`, both lines
   served, the reference regenerated, Golden Hour's 0.2 port.
3. The float frame with HDR on: the frame, the display pass decoding it, and light written instead of marked, one
   group of shaders a pull request (terrain and fluids, particles and the sky, entities and items, the rest).
4. The HUD's own target, the display pass's tone curve and compositing, the 0.1 shim; `keep_world`, `shows_world` and
   the marks removed.
5. On by default: the switch, Smooth Gradients and the `Rgba16Unorm` path removed.

Later, each its own note or pull request: HDR's light-source lift as a built-in preset on 0.2, values for
`shaders/` packs, and which settings stay on the HDR page and which move to the shader menu.

## Open questions

- **The screenshot test's home**: the parity runners (two builds a run) or a synthetic scene with generated textures
  and a committed digest?
- **`[tonemap]` as its own stage**, or the last post pass with a flag?
- **`paper_white` and `peak` in SDR**: 0, or a nominal 80 nits?
- **`shaders/` packs in HDR today**: what the loader does with the frame's alpha, so the change above is stated
  against it.
