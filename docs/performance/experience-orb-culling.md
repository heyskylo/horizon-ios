# Avoiding experience-orb preparation behind the camera

`Game::entity_geometry` used to sample lighting, calculate pulsing color and emit four
vertices for every experience orb, even behind the camera. The shared live/pickup loop now
tests the sprite before that work. Its camera-relative centre is the existing interpolated
or pickup-animation position plus the renderer's 0.1-block Y offset.

A 0.3-block sphere encloses the asymmetric 0.3-scaled quad: the farthest corner is
sqrt(0.15² + 0.225²) blocks from its translated centre. Extra padding covers f32 vertex
rounding. The existing camera frustum decides whether the whole sphere is outside.
Non-finite inputs retain the original geometry path.

A cheap dot product limits eligibility to the billboard's rear hemisphere. This keeps the
ordinary front view from paying for six plane tests per orb. It can retain extra offscreen
sprites, including when a camera hook rotates the actual view; it cannot reject one without
the actual posed-camera frustum also rejecting the padded sphere. This is conservative
rear-view culling, not an attempt to cull every sprite beyond the viewport sides.

The change is automatic and introduces no setting or native-mod-presence restriction.
OIT uses the same check. Shader packs (including native shader packs) retain their existing
geometry path because they can change vertices or views. TAA uses a separate orb-only
frustum with a one-rendered-pixel guard band; other entities keep their existing frustum.
The visible quad, atlas UVs, lighting, color, simulation, pickup state and shadow preparation
keep their existing behavior.

## Measurements

Ryzen 5 7500F, RTX 4060, Linux/Vulkan, optimized `fast` builds, 1280×720, render distance 4,
VSync, OIT and TAA off. Clean baseline main `cc5f7ff23901c104fcdd3f11c9c1ee4e47d557dd`; the
candidate adds the original orb culling. These ordinary measurements precede the TAA
follow-up below; the per-orb preparation/culling logic is unchanged by that follow-up.
The unchanged test module was also moved to the end of its file. Binary and measured-patch hashes are in the data.
All launches use isolated game/HORIZON/XDG directories. No build or tests run during timing.

The same saved world holds 400 frozen Value=7 orbs on a 20×20 grid. Positions:
x = `(column - 9.5) * 1.1 + 0.5`, z = `row * 1.1 + 4.5`, y ≈ 81.4106 after fixture
initialization. Player feet remain `(0.5, 80, -10.5)`, pitch 0°, yaw 180° behind / 0° front.
The front view includes the grid; it does not claim that every orb is unoccluded and onscreen.
The empty control removes the orbs. Every sample verifies the exact entity count, identical
camera feet and zero pending chunk loads/uploads.

Eight launches in A/B/B/A/B/A/A/B order give four adjacent pairs and 24 samples, each with
3 seconds warmup and 8 seconds measured. Simulation stays frozen, apart from two tick steps
when removing entities for the empty control. FPS columns are medians of four sample averages.
Percentage change is the geometric mean of four paired FPS ratios; its approximate 95%
interval uses Student t (3 degrees of freedom), treating launches rather than frames as samples.

| Scene | Baseline FPS | Candidate FPS | Paired change [approx. 95% interval] | Prepare ms |
|---|---:|---:|---:|---:|
| behind | 5956 | 6739 | +12.3% [+8.1, +16.5] | 0.0682 → 0.0582 |
| front | 5530 | 5546 | -0.9% [-9.8, +8.8] | 0.0746 → 0.0747 |
| empty | 7015 | 6656 | -3.6% [-12.7, +6.6] | 0.0532 → 0.0544 |

Only the behind-camera scene establishes an FPS gain. Front and empty controls do not
establish a change in FPS; the front-view median preparation is essentially unchanged.
Behind-camera median 1% lows are 866→887 FPS, so this is not a claim of a comparable
low-percentile gain. These very high FPS values describe a small, CPU-bound rendering
stress scene, not a general multiplier for normal play, live simulation or GPU-bound OIT.
[All CPU/GPU/frame-time samples and checks](experience-orb-culling-samples.json).

## Image and geometry checks

The geometry test covers all four asymmetric corners over 24 yaws, seven pitches and
camera-relative origins including very large coordinates. It checks the padded sphere
against the renderer's quad offsets, including f32 additions to large origins.

Real-client full-frame comparisons cover front, top edge, bottom edge, side edge, behind,
front/top edge with OIT, and both third-person views. Every one of the nine pairs has zero
changed pixels, with no tolerance or mask in the A/B comparison. The quality fixture uses
spectator flight to fix the camera, FOV 70°, fovEffectScale=0 and an asserted camera position.
The edge cases put the sprite centre beyond the unjittered view while part of its quad
remains visible: relative centres `(0,2.11,3)`, `(0,-2.2,3)`, `(3.8,0,3)`.
An independent no-orb control proves nonzero added green sprite pixels in each intended
edge region, rather than accepting two empty images as a successful comparison.

A separate executable loads one native trait mod that observes `camera_pose` and asserts
that one mod is registered. It repeats the nine views above against the ordinary baseline:
all nine full-frame pairs also have zero changed pixels. This checks the loaded-mod path;
FPS samples use ordinary executables without mods.

## TAA support and separate measurements

TAA's projection moves by at most half an input pixel per axis. The orb-only culling
frustum expands by one full input pixel per side, then extracts normalized clip planes.
This covers every jitter phase without modifying the near/far planes or the TAA shader,
jitter sequence or history algorithm. The input dimensions reuse world-post rounding,
resolution scaling and upscaling, including before the first TAA frame or after a resize;
they do not borrow a stale render target or the previous frame's `taa_on` state.

An independent geometry test projects viewport-edge points through actual jittered matrices
at four input resolutions and three camera poses (also with translation/nonuniform scale).
All those points survive the guard, while spheres behind the eye are still rejected. Another
GPU-backed test checks predicted sizes, rounding, zero-size clamping and the first TAA frame.
The Halton test covers all 128 phases allowed by the renderer's maximum upscaling ratio.

The final candidate is measured separately with TAA enabled at native resolution. It uses
the same machine, scene, camera, A/B/B/A/B/A/A/B order and 3+8-second protocol above:

| Scene | Baseline FPS | Candidate FPS | Paired change [approx. 95% interval] | Prepare ms |
|---|---:|---:|---:|---:|
| behind | 3924 | 3880 | -1.5% [-5.8, +2.9] | 0.1064 → 0.0935 |
| front | 3748 | 3750 | +2.1% [-3.6, +8.1] | 0.1119 → 0.1124 |
| empty | 4024 | 3982 | +0.2% [-9.5, +10.9] | 0.0853 → 0.0865 |

These TAA results are separate from the earlier ordinary FPS result. The full sample data
include GPU times and the front/empty controls; a confidence interval spanning zero does
not establish an FPS gain.

TAA's temporal history prevents an unsynchronized screenshot pair from being a bitwise
oracle. The quality run therefore warms each fixed view, captures 80 frames at native TAA
and 192 at Performance (2×) upscaling, and includes an independent repeated launch of the
unmodified baseline. Six views cover the visible edges, front, behind and front-facing
third person; the latter keeps a large visible orb while the cheap hemisphere gate is
eligible, so it exercises the actual frustum check. Observed steady sequences contain
8 native phases and 31–32 upscaled phases per run. Matching selects the closest phase
on a coarse full-frame grid, then measures all pixels and channels of that pair.

Across 239 candidate phase images and 239 baseline-repeat control images, the maximum
channel difference is one 8-bit level in both groups. These are **not** zero-difference
TAA images; per-phase changed-pixel counts and mean differences are retained in the data.
A further third-person TAA run with the loaded native camera-observer mod also differs
by at most one level, as does its independent unmodified-baseline control. Ordinary
non-TAA screenshots from the final TAA-capable executable still match in all nine views
with zero changed pixels.
