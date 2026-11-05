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
4. **No light in alpha** for HDR without a pack and for 0.2 packs: the light-source and HDR-lighting marks give way
   to real values. Alpha keeps only how much the HUD covers, and 0.1 packs keep today's path (below).

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
  `WeatherInput.color`; `Frame` `sky_color` and `fog_color`; `block_texture` and `block_texture_level`; every
  colour texture a stage samples (`scene_color`, `previous`, `history`, pack targets, `opaque_color`, `reflection`,
  `shadow_color`, `block_light_colors`). Data stays data: depth, normals, `light` levels, `emission`, and every
  channel of the LabPBR maps (`block_normal_map`, `block_specular_map` and their `_level` forms).
- **Alpha is coverage only.** The rule to return `input.color.a` goes; a 0.2 post pass's alpha is ignored.
- **Display info in `Frame`**, appended after `shadow_center_texel`:

| Field | Type | Meaning |
| --- | --- | --- |
| `hdr` | `u32` | 1 while the output is HDR, else 0. |
| `headroom` | `f32` | The peak over paper white: the brightest value the display shows (Apple's EDR headroom, Android's HDR/SDR ratio). 1 in SDR. It changes while the game runs (the system's SDR white, a calibration, another display), so read it every frame. |
| `paper_white` | `f32` | Paper white in nits; a nominal 80 in SDR (see the decisions at the end). |
| `peak` | `f32` | The peak brightness in nits; a nominal 80 in SDR, as `paper_white`. |
| `black_level` | `f32` | The display's black in nits (HDR's Black Level); 0 in SDR. |

- **A `[tonemap]` stage**, `fn tonemap(input: TonemapInput) -> vec3<f32>`: the post chain's linear picture in, the
  picture for the display out, linear, 1.0 at paper white and at most `frame.headroom`. It runs once per pixel at
  the window's size, after the post chain and before the camera overlays and the HUD, and owns the look of
  highlights. A pack that leaves it out gets Horizon's:
  in HDR a roll-off into the headroom (ITU-R BT.2390's EETF in PQ; HDR's black level already uses the same
  recommendation's lift), in SDR a clip at white, as vanilla. A pack that only grades keeps Horizon's tone curve;
  one that only tone maps needs no post chain.
- **Horizon's display pass is always last** and can't be replaced: it clamps to the headroom, converts to BT.2020
  for HDR10, lifts the black level, encodes (PQ, scRGB, or 2.2 for SDR) and shows what the HUD and menus cover at
  UI Brightness, as today.

## How the frame stores the picture

The frame depends on what draws the picture, not on the display:

| Picture | Frame | Stored as |
| --- | --- | --- |
| HDR off, no pack | the swapchain's 8-bit format, as today | vanilla's gamma, clipped |
| HDR on, no pack | `Rgba16Float` | vanilla's gamma, values above 1 kept |
| a 0.2 pack (SDR or HDR) | `Rgba16Float` | vanilla's gamma, values above 1 kept; linear light with `[frame] blend = "linear"` |
| a 0.1 pack | as today | as today (see 0.1 packs) |
| a `shaders/`-format pack | as today | as today, with nothing lifted in HDR (see `shaders/`-format packs) |

**When it changes.** Today the frame's format is chosen when the renderer is made and then kept: HDR turned off keeps
the 16-bit frame until the next start, and dropping a pack rebuilds nothing. A float frame can't stay behind like that,
because it doesn't clip as the 8-bit one does. Whenever the row above changes (HDR on or off, a 0.2 pack picked or
dropped, a 0.2 pack swapped for a 0.1 one), the frame and the pipelines that draw into it are rebuilt before the next
frame. What that costs is measured in the series.

**Why gamma storage.** Vanilla blends glass, water, clouds, particles and weather by fixed-function blending in
gamma space. Keeping that encoding keeps every blend of values up to 1 as vanilla's, and solid surfaces and lighting
are the same in either encoding. With HDR on and HDR Lighting off the picture is SDR's except where vanilla would have
clipped at white before a later blend: the sun and moon add onto the sky (`RenderPipelines.CELESTIAL`, src alpha and
one), so clouds or glass over a bright sun come out a little brighter than vanilla's. Goal 1 covers HDR off only;
this case is held to the existing HDR-versus-SDR check (below).

**Linear blending, if a pack asks.** `[frame] blend = "linear"` in the manifest stores linear light instead, for a
physically based pack that wants glass, water and smoke to blend as light does. A 0.2 stage sees the same values
either way; what changes is where Horizon converts:

| | `blend = "gamma"` (the default) | `blend = "linear"` |
| --- | --- | --- |
| The frame holds | vanilla's gamma, values above 1 kept | linear light |
| A 0.2 stage's result | encoded before the frame stores and blends it | stored as it is |
| Horizon's own draws (entities, particles, clouds, weather, stages the pack leaves out) | as today | decoded at the end of their shaders: solid ones look as with gamma, see-through ones blend in linear |
| Horizon's multiplying draws (block breaking, `CRUMBLING_BLEND`) | as today | their factor given in linear terms, so they darken as vanilla's |
| Horizon's fog | mixed in gamma, as vanilla's | mixed in gamma too: encoded, mixed, decoded |
| What a 0.2 stage samples from the frame (`scene_color`, `opaque_color`) | decoded: once before the post chain (a full-screen pass), and in the copy `opaque_color` already is | as stored |

The water mirror and `shadow_color` draw no blends, so they store linear in both.

**Cost and support.**

- `Rgba16Float` is 8 bytes a pixel, as `Rgba16Unorm` is. HDR on keeps two targets at the frame's size, as today:
  the frame, and the HUD's target in place of `keep_world`'s copy, plus the HUD's one-byte coverage.
- A 0.2 pack in SDR moves the frame from 4 to 8 bytes a pixel (about 15 MB more at 1440p), only while it runs. Its
  post chain already uses `Rgba16Float`.
- WebGPU renders to, blends and filters `Rgba16Float` without optional features, unlike `Rgba16Unorm`
  (`draws_wide_frame`); the 8-bit fallback stays only for the HUD's target. The GL backend is checked in the series.
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
  (`entity.wgsl`, `dynamic_light.rs`) and lightning (`gfx/lightning.rs`).
- **The hand** draws at gain 1, as it stays at the world's brightness today, and before `keep_world` (see the HUD
  below), so it needs no mark.

The target look for HDR on without a pack is today's, within the offscreen checks' tolerance, at Auto (203 and 1000
nits). Any change of look is a separate pull request, with screenshots.

Under a 0.2 pack Horizon lifts nothing: the pack gets vanilla's picture in linear light whatever the display, reads
`emission` in its material stage, and writes what shines. (Telling post stages which pixels are light sources is a
likely 0.2 addition, not designed here.)

## The HUD and menus

The HUD always draws in gamma, with vanilla's blends, onto a picture clipped to white, as today:

- **HDR off without a pack, and any 0.1 pack**: onto the frame, as today.
- **A 0.2 pack in SDR**: the display pass writes the tone curve's result to the swapchain in gamma, and the camera
  overlays and the HUD draw on it, as they draw after a pack's post chain today.
- **HDR on**, with or without a 0.2 pack, as below.

With HDR on:

- **Camera overlays belong to the world.** `hud_overlays::camera_overlays` (the vignette, the pumpkin and powder-snow
  overlays and the rest) and the hand draw onto the tone-mapped world before `keep_world` (today just after it),
  converted as Horizon's own draws are under `blend = "linear"`, so they darken or cover light above white too, the
  world border's red vignette included.
- **`keep_world` becomes a pass** instead of a copy. The frame keeps the world, light above white included; the pass
  writes it clipped to white, in gamma, with alpha 0, into the HUD's target in today's HDR frame format
  (`Rgba16Unorm` where the device draws it, else 8-bit). That backdrop is what the HUD draws on, so it clips as today.
- **The HUD and menus keep today's blends, alpha included**, so the crosshair and `GUI_INVERT` (which invert what is
  under them), the text highlight, the menu blur and CIT glint layers (whose blends may read the destination's alpha,
  `hud/cit_glint.rs`) see what they see today; the backdrop's alpha 0 is the alpha `keep_world` leaves today.
- **Coverage has a target of its own**, one 8-bit channel at the same size, cleared to 0, which every HUD draw writes
  as a second output with a blend of its own, only with HDR on: how much of the backdrop is hidden. Draws that lay a
  layer over what is under it (vanilla's translucent GUI blend, and the depth-tested one) add their alpha over it
  (`One, OneMinusSrc` on their alpha); additive draws (the text highlight) leave it, since they hide nothing; the rest
  (the inversion, the End portal fill, CIT glint layers) count as covering what they draw, by their texel's alpha,
  the same way.
- **The display pass composites in gamma**, the space the HUD blends in. With `c` the coverage, `B` the backdrop and
  `P` the HUD's target, the HUD's own part is `H = P - (1 - c) * B`, exact for the layering blends. The output is `H`
  at UI Brightness (`H * ui^(1/2.2)`, so a fully covered pixel is today's covered pixel, scaled by UI Brightness in
  linear light) plus `(1 - c)` of the kept world in gamma, decoded for the display. At the default 100%, wherever the
  world is at or below white, that is `P`, today's picture. Behind see-through HUD the world keeps its light above
  white, weighted as vanilla weights the HUD over it, and doesn't dim with UI Brightness. Inverted pixels count as
  covered, so the crosshair's edges are exact where its texels are opaque.
- **The menu blur keeps light above white.** It blurs the composite so far, instead of the clipped picture, writes the
  result as the new kept world, resets the HUD's target to it, clipped, with alpha 0, and clears coverage to 0. The menu's own darkening then
  layers over it as any HUD does: a bright sun behind a paused game stays bright, dimmed with the rest, so pausing
  doesn't change the picture's brightness.
- `shows_world` and the hand's mark go.

## 0.1 packs: the shim

Horizon serves both lines (`manifest.rs` `API` becomes a list). A 0.1 pack runs on today's path, in SDR and in HDR:

- today's frame (8-bit, or `Rgba16Unorm` with HDR on), inputs and gamma values;
- its post chain through half-float targets that keep values above 1 between passes, onto the frame;
- the marks its stages pass on (`input.color.a`), and the HDR pass lifting light sources on the pack's final colour,
  with HDR lighting off.

That path stays, as a variant of the pipelines that write marks, while 0.1 is served: at least until 0.2 leaves
experimental, and its removal goes in the changelog. Golden Hour stays at 0.1 as its test pack; a 0.2 port sits
beside it.

## `shaders/`-format packs

They keep their own buffers and tone map to SDR in their last program, and their alpha is theirs. Today their
`final` program writes all four channels onto the frame (`gfx/pack_post.rs` `target`), and `keep_world` copies that
alpha into the kept world. With HDR and its HDR Lighting setting on, the HDR pass reads it as marks: below 0.75 it
lifts as a light-source tier, from 0.75 up nothing (the lighting gain is off under a pack). From this note on their
picture is shown at paper white with nothing lifted, as #3125 suggests. HDR values they could read (`hdr`, headroom,
paper white) are a later, optional addition for pack authors.

## Upscaling, anti-aliasing, frame generation

They read the frame as it is stored and, outside the 0.1 path, have no marks to keep, which fixes the alpha problems
above. Native upscaling
([#1403](https://github.com/horizon-rust/horizon/issues/1403)) should take the float frame. TAA over values above 1
may need a tone-weighted resolve so bright pixels don't sparkle. Frame generation stays off with HDR output, as today.

## Tests

1. **HDR off, no pack, bit for bit.** The frame format and pipelines for this case don't change (unit tests on
   `frame_format` and the pipelines' targets). A screenshot test pins the picture: pictures of the game hold
   Minecraft's textures and can't be committed, so it is differential. A runner with a copy of Minecraft and Mesa's
   software Vulkan builds main and the change, captures fixed scenes offscreen with `HORIZON_STILL=1` (repeatable
   frames, [#9398](https://github.com/horizon-rust/horizon/pull/9398), in review), and requires identical PNGs, also
   after turning HDR on and off, picking and dropping a 0.2 pack, and swapping it for a 0.1 one.
2. **Golden Hour (0.1) against main**: the same capture in SDR, and the offscreen HDR capture in HDR, identical.
3. **HDR on, HDR Lighting off, against SDR**: the offscreen HDR capture ([#8947](https://github.com/horizon-rust/horizon/pull/8947)),
   within one 8-bit step outside the spots vanilla clips.
4. **HDR look against today**: the same captures within a tolerance, with screenshots in the pull request,
   including the vignette and the world border's red warning over bright light, the crosshair, chat over a torch,
   a blurred menu, and translucent black and coloured HUD at UI Brightness 50% and 100%.
5. **Cost**: GPU time and memory, HDR on, against today at 1080p and 1440p, on a desktop GPU and an integrated one.
6. **Linear blending**: a 0.2 test pack with `blend = "linear"` draws solid surfaces as with gamma, and glass over
   them as linear blending gives.

## Steps

Behind `HORIZON_HDR_FRAME=float` until the last step, each a small pull request:

1. This note.
2. Shader API 0.2 in `horizon-shader-api`: the rule, the `Frame` fields, `[tonemap]`, `[frame] blend`, both lines
   served, the reference regenerated, Golden Hour's 0.2 port.
3. The float frame with HDR on: the frame, the display pass decoding it, and light written instead of marked, one
   group of shaders a pull request (terrain and fluids, particles and the sky, entities and items, the rest).
4. The tone curve, camera overlays and the hand before `keep_world`, the HUD's target and coverage, the display
   pass's composite, the blur, the rebuild when the frame's case changes, and the 0.1 path split off; marks no
   longer written outside it.
5. On by default and the switch removed. Today's HDR path (marks, the 16-bit unorm frame, Smooth Gradients) then serves
   only 0.1 packs, and goes when 0.1 does.

Later, each its own note or pull request: HDR's light-source lift as a built-in preset on 0.2, values for
`shaders/` packs, and which settings stay on the HDR page and which move to the shader menu. Additions to 0.2 that
break no pack, so they can wait: telling post stages which pixels are light sources, and a full-screen peak for
displays that dim large bright areas below `peak` (Windows reports it, DXGI's `MaxFullFrameLuminance`; other systems
mostly don't).

## Decisions for Ludovic

Each has a recommendation; the step it blocks says when it's needed.

| Decision | Options | Recommended | Why | Needed before |
| --- | --- | --- | --- | --- |
| Where the HDR-off screenshot test runs | **A.** The parity runners: build main and the change, capture fixed scenes with `HORIZON_STILL=1` on Mesa's software Vulkan, compare PNGs. **B.** A synthetic scene with generated textures and a committed digest. | A | It tests the real game with real resource packs; B's digest changes with every Mesa update and covers less. A costs a second build a run, and needs #9398. | Step 3, the first change to the frame |
| How tone mapping is written | **A.** A `[tonemap]` stage of its own. **B.** The last post pass, with a flag. | A | A pack that only grades keeps Horizon's curve, and one that only tone maps needs no post chain; the check can hold the stage to its range (0 to `headroom`); upscaling and anti-aliasing can run between the post chain and the curve. | Step 2, the API |
| `paper_white` and `peak` in SDR | **A.** 80 nits, sRGB's reference white (scRGB's 1.0). **B.** 0, for unknown. | A | A pack that divides by them never gets infinity or NaN; `hdr` = 0 already says they're nominal, and `headroom` is 1 either way. | Step 2, the API |
