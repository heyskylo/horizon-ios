# Reusing unchanged armor-trim preparation

`Game::prepare_trims` scans equipped armor each frame, including entities outside the camera view.
The existing HUD atlas has 32 runtime trim slots. With 33 distinct valid requests, walking them in
order evicts and rebuilds each texture again every frame: PNG decoding, palette generation and
atlas patches repeat even for stationary, hidden armor.

`TrimSlots` keeps the last successfully completed **ordered** request list and the atlas revision.
When both match, it keeps the completed preparation instead of repeating admission. The request
scan still runs, so equipment, material, pattern and equipment-asset override changes are seen.
Changing request order also reruns admission. Atlas replacement, erased resident sprite aliases
and GPU recovery invalidate it. Missing, oversized or unavailable textures retain their retries.

There is no setting or mod-presence restriction. The original texture dimensions, palettes,
32-slot pool and final LRU residents are preserved. In particular this does **not** expand the
pool: more than 32 distinct simultaneous requests already leave earlier trims without a resident
texture. This patch removes repeated preparation; it does not claim to fix that separate capacity
limitation. No gameplay, camera, shader or OIT rendering rules change.

## FPS measurements

Ryzen 5 7500F, RTX 4060, Linux/Vulkan, 1280×720, render distance 4, OIT off, VSync off,
optimized `fast` builds. Both binaries are built from main `a02fdabe6`; the candidate adds only
this change. Every launch uses isolated HORIZON/XDG homes and game directories. No concurrent
build or tests run during measurement, and power settings are untouched.

One saved scene contains 33 invisible marker armor stands, each wearing a diamond helmet with a
different valid material/pattern combination. Invisibility hides the wooden body; worn armor
still renders. Positions: x = `(column - 5) * 1.4`, y = 80, z = `row * 2 + 2.5` (11×3).
The camera is at player feet `(0.5, 80, -9.5)`, pitch 0, yaw 180 for the hidden view and 0
for the front view. The control replaces the final helmet's trim with the first one's, keeping
all 33 equipped entities but reducing distinct requests to 32. The exact combinations and all
24 samples are in [the sample data](armor-trim-preparation-samples.json).

Eight launches, alternating A/B/B/A/B/A/A/B, give four adjacent pairs. Each launch measures the
hidden overflow view, front overflow view and 32-request hidden control, with 3 seconds of
warmup and 8 seconds measured per sample. World simulation is frozen; equipment changes are
followed by two tick steps. Every sample verifies 33 equipped stands and zero pending chunk
loads/uploads. FPS is the median of four sample averages per variant. Percentage change is
the mean of the four paired FPS ratios; its approximate 95% interval uses Student t with 3
degrees of freedom.

| Scene | Baseline median FPS | Candidate median FPS | Paired FPS change (approx. 95% interval) |
|---|---:|---:|---:|
| 33 unique trims, behind camera | 418 | 7594 | +1636.8% (+1343.1…+1930.5%) |
| 33 unique trims, in view | 398 | 4268 | +936.9% (+811.9…+1061.8%) |
| 32 unique trims, behind camera | 6094 | 7606 | +25.0% (+21.4…+28.6%) |

Median frame preparation falls from 2.121 to 0.060 ms behind the camera, from 2.220 to
0.154 ms in front, and from 0.090 to 0.059 ms in the 32-request control. The corresponding
median 1% lows are 367→1967, 345→1676 and 1849→1996 FPS. One candidate launch is slower than
the other three; all four pairs and their intervals are retained.

These gains describe this deliberately concentrated trim workload. They are not a general FPS
multiplier, and no empty-world, native-mod or shader-pack FPS gain is claimed.

## Image and regression checks

The rendering tests use actual generated PNGs and apply emitted atlas patches to CPU pixels.
They check the final resident colors for 33 requests, no new patches on subsequent unchanged
frames, request reordering, replacement equipment, a rebuilt atlas, renderer recovery, erased
sprite aliases and missing/oversized textures becoming available.

Real-client screenshots separately cover 33 different visible trims, changing a helmet's trim,
reducing to 32 distinct trims, OIT, F3+T resource reload and removing all trims. A sequentially
spawned 11×3 wall keeps client entity order equal between binaries. Original ordered comparisons
have 1, 2, 3, 3, 0 and 0 changed pixels respectively. Those first few pixels lie on sprite
boundaries: the original overflow pass continually moves textures among atlas slots, changing
f32 UV rounding. In 40 additional screenshots of the **unchanged baseline**, with no simulation
or camera movement, 33 frames differ from its first image by 1–2 pixels. The candidate overflow
image matches 18 of those baseline frames exactly. This demonstrates pre-existing raster states,
not a new lost trim. After resource reload the compared images are byte-identical. Removing all
trims changes 4,763 pixels by more than 16 levels in either binary, proving visible trim content
was exercised.

An earlier saved-region fixture loaded entities in different orders between launches and changed
309 pixels on two helmets. Order changes which earlier texture exceeds the existing pool.
That fixture is retained as a performance workload, rather than used as a pixel-identity oracle.
Three first diagnostic captures also had a mismatched simulation step; a subsequent aligned
unchanged-baseline capture matched its original reference exactly. No tolerance threshold is
used to label a differing frame as identical.

Local evidence directories: `trim-ab-20261106-134002`, `trim-quality-20261106-134615`,
`trim-quality-ordered-20261106-135302`, `trim-quality-variation-20261106-135511` and
`trim-order-aligned-20261106-135252`. These hold local screenshots and runtime profiles, excluded
from Git. Reproduction drivers are local `trim_harness.py`, `bench_trim.py`, `check_trim.py`,
`check_trim_ordered.py` and `check_trim_variation.py`; public sample data includes all measured
statistics and image comparison counts, but no game assets or saves.
