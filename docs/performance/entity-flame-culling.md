# Offscreen burning-entity flame geometry

`Game::entity_geometry` used to sample world light and expand/upload every burning entity's
billboard before model visibility checks. Reject that work when the flame's own conservative
camera-relative bounds are outside the view. The entity box alone is insufficient: flames
extend above it and have billboard depth. Visibility uses width × 1.4, the maximum half-width,
the full depth sweep over layers and the extra 1.4-scale height of the final layer, with
numerical slack and all yaw rotations covered. Invalid/unusual dimensions keep the original path.

Mobs (including charged blazes), dropped items, armor stands and the visible local body use
the gate before light lookup. Fire simulation, damage, dynamic lights and visible flame geometry
are unchanged. Native mods do not disable the optimization. Shader packs keep the original
path, since they can change vertices or views. TAA uses a guarded billboard
frustum: one input pixel of extra view on each side covers
every jitter phase. Input dimensions follow resolution scaling and upscaling, including
the first frame and resize. This is automatic preparation culling without a new setting.

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
and verify that unsupported dimensions retain the original path. These original measurements
precede the TAA follow-up below. Shader-pack paths still bypass the optimization.

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

## TAA follow-up

The jitter guard expands clip X/Y
by one input pixel per side before extracting normalized planes, keeping near/far unchanged.
The existing flame envelope still bounds all generated layers and yaws. A viewport-edge
regression test checks AABB and sphere acceptance under actual jittered matrices,
across four input sizes and three translated/scaled camera poses.
TAA history, jitter phases, textures, flame tessellation and ordinary model culling are unchanged.

These new measurements isolate sharing that guard with flames: both executables already
include the orb/TAA change and the same main merge (baseline commit `d227d1369`). Their
only runtime difference is the flame follow-up. The current PR separates that same flame
predicate and required jitter guard onto main, without orb culling. These are historical
measurements of the isolated flame change, not new FPS measurements of the split branch.
This is a separate dataset from the original
TAA-off results above. Linux/Vulkan, Ryzen 5 7500F / RTX 4060, optimized `fast`, 1280×720,
render distance 4, native TAA on, OIT/VSync off, ordinary entity shadows on. The same frozen
400-cow save, camera feet `(0.5,80,-9.5)`, pitch 8° and front/behind yaws 0°/180° are used.
The empty control copies the same save with entity regions removed. Every sample checks
all 400 mobs burning (or zero for empty), no particles, unchanged camera and no chunk backlog.
Four adjacent A/B pairs in A/B/B/A/B/A/A/B order; 3 seconds warmup + 8 measured; no other
game, build or tests during timing. Median FPS and preparation; paired log-ratio Student-t
intervals with 3 degrees of freedom, treating launches as the sampling unit:

| Scene | Baseline FPS | Candidate FPS | Paired change [approx. 95% interval] | Prepare ms |
|---|---:|---:|---:|---:|
| behind | 3325 | 3690 | +11.2% [+6.5, +16.1] | 0.1530 → 0.1299 |
| front | 990 | 1033 | +2.7% [-3.1, +8.9] | 0.8312 → 0.7963 |
| empty | 3924 | 3783 | -3.3% [-7.5, +1.0] | 0.0894 → 0.0910 |

Only the behind-camera scene establishes an FPS gain. Front and empty intervals span
zero; the empty estimate trends lower, so these results do not establish zero overhead.
Behind-camera median 1% lows are 656→665 FPS, without a comparable low-percentile gain.
These are concentrated rendering workloads rather than a general FPS multiplier. All frame/GPU/CPU statistics, 1% lows, hashes, controls and per-phase differences
are retained in [the separate TAA dataset](entity-flame-taa-samples.json).

Eight real-client views cover front, top edge, side edge, above an offscreen body, the same
edge with OIT, behind, and both third-person cameras. The invisible cow is at `(0.5,100,4.5)`;
the above-body view translates the original edge fixture upward by 21 blocks (eye y=102.62,
distance 3, pitch −25°). Its padded body is outside the viewport, while thousands of warm
flame pixels remain. Independent Fire=0 controls confirm added warm pixels in every intended
visible view. Fixed spectator cameras, FOV 70° and asserted feet prevent player drift.

TAA quality captures 80 native frames and 192 frames with Performance (2×) upscaling after
warming each view, with a separately repeated unmodified baseline. The steady sequences
contain 8 native and 30–32 upscaled phases. Matching chooses the nearest phase using a
stride-8 full-frame grid, then measures every pixel/channel without a mask.

All 319 candidate phase comparisons and 317 independent baseline-repeat comparisons
have maximum channel difference 2 and 2 respectively (8-bit levels). These are
not bitwise-identical TAA images. The 2-level maximum occurs only in the front-facing
Performance-upscaled view, in both candidate and baseline control; every other view
differs by at most one level. A further native-TAA run with one loaded camera-observer
mod checks front, above-body edge and front-facing third person; candidate and independent
baseline repeat also differ by at most 1 level. The ordinary TAA-off executable checks all
eight views against the same baseline: every full-frame pair has zero changed pixels.
