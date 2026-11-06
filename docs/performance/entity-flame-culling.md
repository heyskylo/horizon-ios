# Offscreen burning-entity flame geometry

`Game::entity_geometry` used to sample world light and expand/upload every burning entity's
billboard before model visibility checks. Reject that work when the flame's own conservative
camera-relative bounds are outside the view. The entity box alone is insufficient: flames
extend above it and have billboard depth. Visibility uses width × 1.4, the maximum half-width,
the full depth sweep over layers and the extra 1.4-scale height of the final layer, with
numerical slack and all yaw rotations covered. Invalid/unusual dimensions keep the original path.

Mobs (including charged blazes), dropped items, armor stands and the visible local body use
the gate before light lookup. Fire simulation, damage, dynamic lights and visible flame geometry
are unchanged. Native mods do not disable the optimization. Shader packs and TAA keep the
original path; TAA's jittered projection can extend beyond this camera frustum. This is
automatic preparation culling without a new setting.

## Measurement

Base `2b74332fb`, Linux/Vulkan/X11, Ryzen 5 7500F, RTX 4060, 1920×1080, distance 4,
VSync off/unlimited FPS, OIT off, ordinary entity shadows enabled, TAA off. Neither the separate
blob-shadow PR nor the name-tag cache is in these builds. Existing desktop apps and power
settings were left alone. Fresh isolated homes/game folders, one copied save, fixed time/weather,
frozen server ticks; no other game, build or test ran during measurement.

400 cows in a 20×20 grid, 1.1-block spacing, y=80, NoAI/NoGravity/Silent/Invulnerable,
Fire=32767. After two initial server ticks, freeze and save. Each launch verifies 400 mobs,
all 400 burning on the server, no particles and no streaming backlog. Camera feet
(0.5,80,-9.5), pitch 8°, yaw 180° away / 0° toward them. F1 hides HUD in both builds.
The empty control removes only entity regions from a scratch copy. A/B/B/A/B/A/A/B order,
3 s warmup + 8 s measured: four pairs / 24 samples. Medians of four runs:

| Scene | Baseline FPS | Culled FPS | Paired gain (approx. 95% t interval) | Prepare ms, before → after |
|---|---:|---:|---:|---:|
| behind | 3329 | 4190 | +23.7% [+10.7, +38.2] | 0.167 → 0.136 |
| front | 1050 | 1039 | -1.7% [-6.8, +3.7] | 0.797 → 0.805 |
| empty | 5257 | 5180 | -2.3% [-7.5, +3.2] | 0.074 → 0.074 |

The t interval uses four paired log FPS ratios (critical 3.182), not frames as independent
samples. This rendering-stress result applies to many offscreen burning entities; it does
not predict a general FPS gain, live mob simulation or GPU-bound scenes. Front and empty
controls show the cost where nothing useful can be culled.
[Raw CPU/GPU/FPS/1% lows and summaries](entity-flame-culling-samples.json).

## Correctness

A separate invisible burning cow exposes the flame without its model: feet (0.5,79,-2.5),
camera feet (0.5,80,-5.5), eye y=81.62, vertical FOV 70°, pitch −25°. Even the body's
0.5-block-padded box is below the viewport: its highest angle is −10.33°, below the
−10° bottom plane. Yet 3755 orange flame pixels remain. Setting Fire=0 produces zero
orange pixels (R>80, G>25, R>1.25G, B<0.6G); this independent control verifies
visible flames in the reference scene.

Five baseline/candidate comparisons at 1280×720 were identical: front, above the body,
the same edge with OIT enabled, a side view, and behind. Unit tests bound actual emitted
vertices for seven dimensions, three scales, 24 yaws and cameras near the world border,
and verify that unsupported dimensions retain the original path. Shader-pack/TAA paths bypass
the optimization; no shader-pack or TAA rendering gain is claimed.

Armor-stand flame reception is not used as a benchmark: an existing shared-flag decoding gap
in `remote_objects.rs` ignores its burning bit. This change does not address that separate bug.

The dense front scene has two raster states differing in 236 pixels in a small overlap.
Both occur in these unmodified-baseline launches, and each candidate state exactly matches
one baseline state. The separate single-flame comparisons above provide the pixel oracle.

A follow-up compatibility run enabled one native trait mod through `entry::main_with`.
Its camera hook asserted that the mod was loaded and observed the camera without changing it.
All five views above matched the updated ordinary executable exactly, including 3755 visible
flame pixels above the offscreen body. Native mods therefore no longer cause a blanket bypass.
The FPS samples above were collected without mods; this run checks rendering compatibility,
not the behavior of arbitrary mixins that rewrite the flame generator or rendering passes.
