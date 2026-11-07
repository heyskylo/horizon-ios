# Skipping falling-block preparation behind the camera

Falling blocks previously sampled light and rebuilt their baked-model vertices even when
all their geometry was behind the camera. In a separate baseline profiling run with 512
frozen sand entities, `eg_objects` took about 0.24 ms of 0.29 ms median frame preparation.
Tracing was collected after the FPS sample rather than enabled during timing.

The camera-relative model origin keeps the existing interpolation, centering and random
block-model offset. Only rear-origin models are eligible for an envelope/depth check;
front-origin models retain the ordinary preparation path. Eligibility alone never rejects
anything. The actual posed-camera near/far clip planes must reject the entire padded model.
Side-of-screen geometry is retained, as are extra models that the eligibility gate skips.
This is conservative rear-view preparation culling, not general entity visibility culling.

The envelope comes from every baked face in every weighted/multipart alternative of the
state's definition, including transformed models and resource-pack geometry outside a unit
cube. It can be wider than the model picked by one seed, which only retains extra work.
No render random draws change. Envelopes are cached lazily per catalog; `models_mut` clears
the cache before mutable geometry access, and resource replacement owns a fresh catalog.
Empty, unbound or non-finite geometry keeps the original path. Padding covers f32 additions.

Depth clipping is independent of TAA's X/Y jitter, including upscaling and resolution
changes; this leaves the TAA shader/history/jitter sequence and the global frustum unchanged.
The optimization is automatic, with no setting or native-mod-presence bypass. Shader packs,
including native shader packs, keep the original geometry path because they may change
vertices or cameras. Existing same-state suppression, lighting, UVs, tint, model selection,
simulation, collisions and block landing are unchanged.

## Measurements

Ryzen 5 7500F / RTX 4060, Linux/Vulkan, optimized `fast`, 1280×720, render distance 4,
VSync and OIT off. Clean main `a1ff75a4a525b6a9e1ed68f74cf9d54161596370` versus that same
base with this change. Exact source-patch and executable hashes are in the data. Each game
uses isolated game/HORIZON/XDG folders; no other game, build, tests or image analysis runs
during timing. Existing desktop applications and power settings are left alone.

One saved scene contains 512 frozen NoGravity falling-sand entities: 32 columns × 16 rows,
x=`(column-15.5)*0.7+0.5`, z=`row*0.7+4.5`, y=82. Player feet `(0.5,80,-10.5)`, pitch 8°,
yaw 180° away or 0° toward them. The front view includes the grid, without claiming every
block is visible/unoccluded. Empty removes entity regions from a copy of the same save.
Each sample verifies 512/0 falling blocks on the server, unchanged feet, no particles and
zero pending chunk loads/uploads. Visible-block controls separately verify client rendering.

Each mode has 24 samples over eight cycles (16 launches), A/B/B/A/B/A/A/B, with four adjacent
pairs and three scenes per cycle. Each scene warms for 3 seconds and measures 8 seconds.
FPS columns are medians of four sample averages; percentage change is the geometric mean
of paired ratios. Approximate 95% intervals use Student t on four paired log ratios (3 df),
treating launches rather than frames as independent samples.

### TAA off

| Scene | Baseline FPS | Candidate FPS | Paired change [approx. 95% interval] | Prepare ms |
|---|---:|---:|---:|---:|
| behind | 2107 | 5332 | +163.4% [+136.5, +193.3] | 0.3134 → 0.0710 |
| front | 2012 | 2062 | +3.1% [-0.2, +6.6] | 0.3211 → 0.3172 |
| empty | 6143 | 5735 | -5.0% [-14.4, +5.5] | 0.0565 → 0.0578 |

Behind-camera median 1% lows: 511→600 FPS.

### Native TAA on

| Scene | Baseline FPS | Candidate FPS | Paired change [approx. 95% interval] | Prepare ms |
|---|---:|---:|---:|---:|
| behind | 1875 | 3400 | +83.5% [+70.2, +97.8] | 0.3356 → 0.1053 |
| front | 1826 | 1828 | +0.9% [-5.2, +7.3] | 0.3389 → 0.3366 |
| empty | 3294 | 3282 | -0.9% [-11.4, +10.9] | 0.0914 → 0.0911 |

Behind-camera median 1% lows: 377→419 FPS.

These are concentrated rendering workloads, not a general FPS multiplier. An interval
spanning zero does not establish an FPS change or prove zero overhead. All 48 complete
frame/CPU/GPU statistics, low percentiles, control estimates and image differences are
retained in [the sample data](falling-block-depth-culling-samples.json).

The initial prototype tested bounds for front-origin blocks too. A separate four-pair run
found a 5.4% front-view loss (approx. 95% interval -10.2…-0.4%). That prompted the cheap
eligibility gate; the tables above are a new complete dataset from the refined executable.

## Geometry and image checks

A regression test bakes original synthetic geometry extending outside a unit cube and
checks actual weighted/multipart selected faces over 128 position seeds, including rotated
variants. It also checks mutable geometry invalidation, catalog replacement and invalid or
empty models. Another test maps actual jittered viewport-edge points back into world space
at four input sizes and three camera poses: the depth test retains them, rejects boxes
behind/outside depth range, and retains geometry straddling the eye. Existing gameplay tests
cover the unchanged interpolated/random-offset model origin and same-state suppression.

Eight ordinary real-client views compare the final executable with the clean baseline:
front, top/bottom/side visible edges, behind, front with OIT, and both third-person cameras.
All eight full-frame pairs have zero changed pixels, with no mask or tolerance. Fixed
spectator cameras, FOV 70° and asserted feet prevent camera drift. Independent no-block
images verify over 1,000 added sand-colored pixels in each intended visible view; that
control predicate does not mask any A/B comparison.

A separate original resource-pack fixture forces an unrotated sand model with geometry
`[0,0,0]..[16,16,32]` (model units), referencing the player's existing sand texture. With
entity `(0.5,100,4.5)` and camera eye `(0.5,100.5,5.5)`, a unit cube would be wholly behind
the near plane, but the extended model remains visible. This and front/third-person-front
views all match exactly; the independent no-block control verifies hundreds of thousands
of added sand pixels in the critical behind-origin case. The original fixture JSON is in
the data; no game textures or prepared resources are included.

TAA quality warms each fixed view and captures 80 native frames / 192 Performance (2×)
upscaled frames. It includes an independent repeated baseline. Closest-phase matching uses
a stride-8 full-frame grid, then assesses every pixel/channel without a mask. These temporal
images are reported separately from ordinary bitwise equality.

The TAA run compares 160 candidate phase images and 160 baseline-repeat images;
maximum 8-bit channel difference is 1 and 1 respectively.
A separately loaded native camera-observer mod asserts one registered mod and observes the
actual camera. Its TAA front/behind/third-person-front checks reach a maximum difference of
1 level, versus 1 for its repeated-baseline control.
All per-phase changed-pixel counts and means are retained rather than claiming bitwise TAA
equality or universal compatibility with arbitrary rendering mixins.
