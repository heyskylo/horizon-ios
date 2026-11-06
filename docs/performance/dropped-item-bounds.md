# Dropped-item bounds preparation

The renderer used to compute the bob's local bounds, compute those bounds again for stack-copy placement,
then transform every model point under every copy to make an occlusion envelope. With Entity Culling off
(the default), the envelope's only consumer immediately returned `Bypass`.

The draw now reuses its already computed local depth. `cluster_poses` remains available with its original
signature; the depth-taking helper preserves its arithmetic and RNG order. With Entity Culling off and
TAA off, the expensive posed-envelope walk is skipped. Distance and frustum checks, model resolution,
copy count, geometry, lighting, glint and animation remain in place. When culling is enabled, the original
posed-envelope and occlusion path remains in use, including custom resource-pack models.

TAA retains that posed-envelope walk. The first version skipped it with TAA too: in eight paired runs on
256 visible stacks it raised mean FPS but lowered 1% lows by 6.82% overall (five negative pairs, three positive).
The final version applies only local-depth reuse under TAA. This is a measured performance guard, not a fix
for that TAA tail regression; its full cause is not diagnosed here. No FPS cap or graphics-quality setting is changed.

## Production game measurements

Base: `9d11d0c710705ee0901c2186d78641048a6660b0`. Separate immutable optimized binaries, native Wayland /
Vulkan, RTX4060 / Ryzen7500F, 1920x1080, ordinary seed7 world, render distance 4, VSync/OIT/dynamic lights off.
Frozen ticks and visual clock, noon. 0, 32 or 256 separate item entities, each a stack of 64 diamonds. The
256 stacks fit within the default item draw range, in a 16x16 grid at 0.4-block spacing. Camera
(.5,110,-7.5), yaw 0, pitch 8. All game data and settings are isolated from real player data.

Each sample is a fresh game process loading a finalized save whose population was checked after a separate
reload. Three seconds of warmup, twelve seconds of untraced FPS timing; ABBA/BAAB order. Four pairs per
TAA state for 256 stacks and two pairs per state for controls. No builds, tests or CPU probes overlap timing.
Camera, viewport, population, settled queues and absence of physical-input changes are asserted. Unrelated
applications remain open and GPU clocks are not locked. Item bob phases can vary with runtime entity IDs;
these are repeated workload comparisons rather than byte-identical frame-input captures.

| Stacks | TAA | Mean FPS, before / after | Mean change | 1% low FPS, before / after | 1% low change |
|---:|:---:|---:|---:|---:|---:|
| 256 | off | 1099 / 1691 | +53.90% | 693 / 859 | +23.98% |
| 256 | on | 1083 / 1143 | +5.55% | 756 / 768 | +1.77% |
| 32 | off | 4378 / 4500 | +2.78% | 1098 / 1110 | +1.08% |
| 32 | on | 2204 / 2217 | +0.59% | 731 / 733 | +0.24% |
| 0 | off | 5580 / 5570 | -0.18% | 1199 / 1183 | -1.38% |
| 0 | on | 2399 / 2394 | -0.20% | 808 / 795 | -1.58% |

Changes are geometric means of within-pair ratios. Absolute columns average each version's samples.
For 256 stacks with TAA off, all four pairs improved both mean FPS and 1% lows. With TAA on,
the four 1%-low changes were approximately 0.0%, -5.1%, +8.7% and +3.9%; no consistent tail-latency
improvement is established. The small-population and empty-scene 1%-low changes were also mixed.
Individual pairs, frame percentiles, slow-frame phases, binary hashes and the earlier unrestricted version
are retained in [the raw report](dropped-item-bounds-samples.json). These frozen stress-scene results are
not an ordinary-exploration FPS multiplier. A small item population may be GPU/presentation limited;
do not extrapolate the 256-stack result to a few items or infer a guaranteed 1%-low gain from CPU savings.

## Output checks and limits

An independent executable linked against the original and final libraries compared 2160 stack-pose cases:
flat/deep models, the depth threshold and its adjacent float, zero through five copies, multiple RNG seeds,
translations, rotations and mirrored/nonuniform transforms. All 354240 output bytes matched. A unit test
also pins original flat and solid random-copy placements. Existing CPU/GPU item parity tests are retained.

The actual-game visual fixture contains eight ordinary, transparent, foil and special models, a glowing
item, and a wall with a gap. It tests Entity Culling off, synchronous and asynchronous, with TAA off/on.
Runtime entity IDs are aligned to 128..135 to preserve item bob phases; items start clear of the wall to
avoid random collision escape. Without TAA, the full RGB image is identical in all three culling modes.
The TAA jitter phase/history is not synchronized across processes: those captures differ and cannot
establish pixel equality or independently rule out a TAA image regression. Their maximum channel
differences are 118 (off), 112 (synchronous) and 149 (asynchronous), on a 0..255 scale. The independent
stack-transform check still covers the shared arithmetic. All six pixel comparisons are in the raw report.

The project's Golden Hour 1.0.0 shader pack also loaded and rendered successfully in four independent
before/after games, with the TAA setting off/on and Entity Culling off. Both pairs' full RGB images
matched. Golden Hour has its own post-processing: the renderer already bypasses built-in TAA under
that post pass, so this does not exercise an active built-in TAA/shader combination. This is a
compatibility check, not a shader FPS measurement; other packs and shader shadow passes were not
exercised by this fixture. The changed CPU preparation is shared by shader rendering; no shader-pack
exclusion is added. The conservative guard follows the TAA setting even when a pack bypasses that pass.

Earlier visual trials with different runtime IDs or items intersecting the wall were rejected because
their input poses/positions differed. Initial FPS trials used saves copied while the game was still
open; their population checks rejected the empty reloads. Those trials are excluded from measurement
claims and retained locally. The final saves were copied only after shutdown and independently reopened.

## Local self-check

`python3 tools/pr_ready.py --base upstream/main` passed: 3234 Rust tests, 712 Python tests, rustfmt,
workspace/all-targets clippy with warnings denied, the no-default-features client clippy check, doc tests
and the public-tree check. Java-oracle parity harnesses did not run because their vanilla data is not
prepared here; CI selects the affected harnesses. The report was finalized after that run without
changing either measured Rust source file; the final public tree and whitespace were checked again.

Only project code and locally prepared game data were inspected; no other project's code was read or
copied. Prepared models, textures, saves and captures are private local validation artifacts and are not
included in this report. Linux Vulkan is the tested renderer; other drivers/platforms were not run.
